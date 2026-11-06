//! Recursive search over the current folder.
//!
//! Modelled on GNOME Files (nautilus): there, a search is just another
//! `NautilusDirectory`, so the view does not special-case it — the engine emits
//! hits, the directory turns them into files, and the same model chain renders
//! them (`src/nautilus-search-directory.c`, `src/nautilus-search-engine.c`).
//!
//! This module implements the equivalent of nautilus' `model` search provider
//! (`nautilus-search-engine-model.c`): walk the folder and its subdirectories
//! and keep the entries whose name matches. Nautilus' other providers
//! (Tracker/`localsearch`, recent files, full-text) need an index or a service
//! and are out of scope for now.

use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use anyhow::Result;
use fs::Fs;
use futures::{StreamExt, future::join_all};
use gpui::SharedString;

use crate::model::entries::{DirEntry, SortMode, sort_entries};

/// How deep the walk descends below the searched folder.
///
/// Nautilus leaves this to the "search in subfolders" preference; a fixed
/// bound keeps a stray search in `/` from walking forever.
const MAX_DEPTH: usize = 8;

/// Upper bound on collected hits, so a broad query stays responsive.
pub(crate) const MAX_RESULTS: usize = 2000;

/// The outcome of one search.
pub(crate) struct SearchResults {
    /// The matching entries, sorted like the folder view.
    pub(crate) entries: Vec<DirEntry>,
    /// Whether the walk stopped early because `MAX_RESULTS` was reached.
    pub(crate) truncated: bool,
}

/// What a search looks at: GNOME Files' query editor filters
/// (`nautilus-query.c`, `NautilusQuery:recursive` / `:search-content`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchOptions {
    /// Descend into subfolders. `false` searches the open folder only.
    pub(crate) recursive: bool,
    /// Also match files whose *content* contains the query.
    pub(crate) contents: bool,
    /// Restrict the hits to one kind of file.
    pub(crate) kind: SearchKind,
    /// Only entries modified within this many days.
    pub(crate) modified_within_days: Option<u32>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            contents: false,
            kind: SearchKind::Any,
            modified_within_days: None,
        }
    }
}

/// The file kinds the search can be limited to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchKind {
    #[default]
    Any,
    Images,
    Documents,
    Code,
}

impl SearchKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SearchKind::Any => "Any type",
            SearchKind::Images => "Images",
            SearchKind::Documents => "Documents",
            SearchKind::Code => "Code",
        }
    }

    /// Whether a file with this extension belongs to the kind.
    fn matches(self, path: &std::path::Path) -> bool {
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            return self == SearchKind::Any;
        };
        let list: &[&str] = match self {
            SearchKind::Any => return true,
            SearchKind::Images => &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"],
            SearchKind::Documents => &[
                "md", "txt", "pdf", "odt", "doc", "docx", "rtf", "csv", "xls", "xlsx", "ppt",
                "pptx",
            ],
            SearchKind::Code => &[
                "rs", "c", "h", "cpp", "hpp", "cc", "py", "js", "ts", "tsx", "jsx", "go", "java",
                "rb", "sh", "toml", "json", "yaml", "yml", "nix", "lua", "zig",
            ],
        };
        list.contains(&extension.as_str())
    }
}

/// Files larger than this are not read for a content search.
///
/// Full-text search in GNOME Files goes through an index; reading every file
/// would stall on a single huge file, so the walk skips files this large.
const MAX_CONTENT_SEARCH_BYTES: u64 = 4 * 1024 * 1024;

/// Searches `root` and its subdirectories for entries whose name (or content)
/// contains `query`, case-insensitively.
pub(crate) async fn search_directory(
    fs: Arc<dyn Fs>,
    root: PathBuf,
    query: String,
    show_hidden: bool,
    sort_mode: SortMode,
    options: &SearchOptions,
) -> Result<SearchResults> {
    let needle = query.to_lowercase();
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut found: Vec<(PathBuf, String, bool)> = Vec::new();
    let mut truncated = false;

    'walk: while let Some((directory, depth)) = queue.pop_front() {
        let mut stream = match fs.read_dir(&directory).await {
            Ok(stream) => stream,
            // An unreadable subdirectory must not abort the whole search.
            Err(_) => continue,
        };

        while let Some(path) = stream.next().await {
            let Ok(path) = path else { continue };
            let Some(name) = path.file_name() else {
                continue;
            };
            let name = name.to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            if !show_hidden && name.starts_with('.') {
                continue;
            }

            let is_dir = fs.is_dir(&path).await;

            // Folder hits are never filtered by kind or date: a folder is how
            // the user reaches the files inside it.
            let metadata =
                if !is_dir && (options.modified_within_days.is_some() || options.contents) {
                    fs.metadata(&path).await.ok().flatten()
                } else {
                    None
                };

            let mut hit = name.to_lowercase().contains(&needle);
            if !hit && options.contents && !is_dir {
                let small_enough = metadata
                    .as_ref()
                    .is_none_or(|metadata| metadata.len <= MAX_CONTENT_SEARCH_BYTES);
                if small_enough && let Ok(text) = fs.load(&path).await {
                    hit = text.to_lowercase().contains(&needle);
                }
            }

            let matches_kind = is_dir || options.kind.matches(&path);
            let matches_date = match (options.modified_within_days, metadata.as_ref()) {
                (Some(days), Some(metadata)) => {
                    let cutoff = std::time::SystemTime::now()
                        - std::time::Duration::from_secs(days as u64 * 24 * 60 * 60);
                    metadata.mtime.timestamp_for_user() >= cutoff
                }
                // Without metadata the filter cannot be applied; keep the hit
                // rather than hiding a file the user can see.
                (Some(_), None) => true,
                (None, _) => true,
            };

            if hit && matches_kind && matches_date {
                if found.len() >= MAX_RESULTS {
                    truncated = true;
                    break 'walk;
                }
                found.push((path.clone(), name, is_dir));
            }

            if is_dir && options.recursive && depth + 1 <= MAX_DEPTH {
                queue.push_back((path, depth + 1));
            }
        }
    }

    let entries = join_all(found.into_iter().map(|(path, name, is_dir)| {
        // The async block must own `is_dir`, `path` and `name`, so clone the
        // `Arc` instead of borrowing it into the future.
        let fs = fs.clone();
        async move {
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
                name: SharedString::from(name),
                is_dir,
                len,
                mtime,
            }
        }
    }))
    .await;

    let mut entries: Vec<DirEntry> = entries;
    sort_entries(&mut entries, sort_mode);

    Ok(SearchResults { entries, truncated })
}
