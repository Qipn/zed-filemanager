//! One row in the file manager list and its directory reader.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use fs::{Fs, MTime};
use futures::{StreamExt, future::join_all};
use gpui::SharedString;

/// One row in the file manager.
#[derive(Clone)]
pub(crate) struct DirEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: SharedString,
    pub(crate) is_dir: bool,
    /// File size in bytes; `None` when unavailable or for directories.
    pub(crate) len: Option<u64>,
    /// Modification time; `None` when unavailable.
    pub(crate) mtime: Option<MTime>,
}

impl DirEntry {
    /// Names starting with `.` are hidden on Unix.
    pub(crate) fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}

/// How entries are ordered. Directories always group before files; the
/// selected key and direction apply within each group.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum SortMode {
    #[default]
    NameAscending,
    NameDescending,
    ModifiedAscending,
    ModifiedDescending,
    SizeAscending,
    SizeDescending,
}

impl SortMode {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            SortMode::NameAscending => "Name ↑",
            SortMode::NameDescending => "Name ↓",
            SortMode::ModifiedAscending => "Modified ↑",
            SortMode::ModifiedDescending => "Modified ↓",
            SortMode::SizeAscending => "Size ↑",
            SortMode::SizeDescending => "Size ↓",
        }
    }

    /// Whether the entries are sorted by name.
    pub(crate) fn is_name(self) -> bool {
        matches!(self, SortMode::NameAscending | SortMode::NameDescending)
    }

    /// Whether the entries are sorted by modification date.
    pub(crate) fn is_modified(self) -> bool {
        matches!(
            self,
            SortMode::ModifiedAscending | SortMode::ModifiedDescending
        )
    }

    /// Whether the entries are sorted by size.
    pub(crate) fn is_size(self) -> bool {
        matches!(self, SortMode::SizeAscending | SortMode::SizeDescending)
    }

    /// The same sort key with the opposite direction.
    pub(crate) fn toggled(self) -> Self {
        match self {
            SortMode::NameAscending => SortMode::NameDescending,
            SortMode::NameDescending => SortMode::NameAscending,
            SortMode::ModifiedAscending => SortMode::ModifiedDescending,
            SortMode::ModifiedDescending => SortMode::ModifiedAscending,
            SortMode::SizeAscending => SortMode::SizeDescending,
            SortMode::SizeDescending => SortMode::SizeAscending,
        }
    }
}

/// Reads the entries of `directory`, collects metadata for non-directories and
/// filters hidden entries when `show_hidden` is false.
pub(crate) async fn read_entries(
    fs: Arc<dyn Fs>,
    directory: PathBuf,
    show_hidden: bool,
    sort_mode: SortMode,
) -> Result<Vec<DirEntry>> {
    let mut stream = fs.read_dir(&directory).await?;
    let mut paths = Vec::new();

    while let Some(path) = stream.next().await {
        let path = path?;
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        paths.push((path, name));
    }

    let entries = join_all(paths.into_iter().map(|(path, name)| async {
        let is_dir = fs.is_dir(&path).await;
        let metadata = if is_dir {
            None
        } else {
            fs.metadata(&path).await.ok().flatten()
        };
        let (len, mtime) = match &metadata {
            Some(metadata) => (Some(metadata.len), Some(metadata.mtime)),
            None => (None, None),
        };
        DirEntry {
            path,
            name: name.into(),
            is_dir,
            len,
            mtime,
        }
    }))
    .await;

    let mut entries: Vec<DirEntry> = entries
        .into_iter()
        .filter(|entry| show_hidden || !entry.is_hidden())
        .collect();

    sort_entries(&mut entries, sort_mode);

    Ok(entries)
}

/// Sorts `entries` in place: directories group first, then by the selected
/// key, case-insensitively for names.
pub(crate) fn sort_entries(entries: &mut [DirEntry], sort_mode: SortMode) {
    let key_order = |left: &DirEntry, right: &DirEntry| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| match sort_mode {
                SortMode::NameAscending | SortMode::NameDescending => {
                    let ordering = left.name.to_lowercase().cmp(&right.name.to_lowercase());
                    match sort_mode {
                        SortMode::NameDescending => ordering.reverse(),
                        _ => ordering,
                    }
                }
                SortMode::ModifiedAscending | SortMode::ModifiedDescending => {
                    let ordering = match (left.mtime, right.mtime) {
                        // `bad_is_greater_than` is fs' sanctioned (if
                        // discouraged) way to compare modification times.
                        (Some(left), Some(right)) => right
                            .bad_is_greater_than(left)
                            .cmp(&left.bad_is_greater_than(right)),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    };
                    match sort_mode {
                        SortMode::ModifiedDescending => ordering.reverse(),
                        _ => ordering,
                    }
                }
                SortMode::SizeAscending | SortMode::SizeDescending => {
                    let ordering = left.len.unwrap_or(0).cmp(&right.len.unwrap_or(0));
                    match sort_mode {
                        SortMode::SizeDescending => ordering.reverse(),
                        _ => ordering,
                    }
                }
            })
    };
    entries.sort_by(key_order);
}

/// Formats a byte count for the status bar: B / KiB / MiB / GiB.
pub(crate) fn format_len(bytes: u64) -> String {
    const KIB: f64 = 1024.;
    const MIB: f64 = KIB * 1024.;
    const GIB: f64 = MIB * 1024.;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}
