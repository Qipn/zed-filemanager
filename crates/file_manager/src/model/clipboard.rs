//! The panel's clipboard state and file-operation helpers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use fs::{CopyOptions, Fs};

/// The panel's internal clipboard for copy/cut/paste. Holds every selected
/// path so multi-selection can be pasted as a batch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Clipboard {
    Copied(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    /// Paste creates hard links instead of duplicating the data.
    Linked(Vec<PathBuf>),
}

impl Clipboard {
    /// The paths this clipboard entry refers to.
    pub(crate) fn paths(&self) -> &[PathBuf] {
        match self {
            Clipboard::Copied(paths) | Clipboard::Cut(paths) | Clipboard::Linked(paths) => paths,
        }
    }
}

/// Active inline rename: the entry being renamed.
///
/// The typed text lives in the panel's [`crate::ui::search_field::SearchField`]
/// entity, so the inline editor gets a real caret, selection and input-method
/// support (the rename row renders that entity in place of the name). The panel
/// keeps only the path: `FileManagerPanel::rename_state: Option<PathBuf>`.

/// Copies a file, or a directory tree via `fs::copy_recursive`.
pub(crate) async fn copy_entry(fs: &Arc<dyn Fs>, source: &Path, target: &Path) -> Result<()> {
    if fs.is_dir(source).await {
        fs::copy_recursive(
            fs.as_ref(),
            source,
            target,
            CopyOptions {
                overwrite: false,
                ignore_if_exists: true,
            },
        )
        .await
    } else {
        fs.copy_file(
            source,
            target,
            CopyOptions {
                overwrite: false,
                ignore_if_exists: true,
            },
        )
        .await
    }
}

/// Creates `target` as another name for `source` instead of copying the bytes.
///
/// `link(2)` refuses directories, so a directory is mirrored instead: every
/// directory is created and every file inside is hard linked (what `cp -al`
/// does). The tree walk reads through `fs` so it works with the fake filesystem
/// in tests; the link syscall itself has no equivalent in `Fs` and is called on
/// `std::fs` — it is a single metadata operation, never a data copy.
pub(crate) async fn link_entry(fs: &Arc<dyn Fs>, source: &Path, target: &Path) -> Result<()> {
    if fs.is_dir(source).await {
        let items = fs::read_dir_items(fs.as_ref(), source).await?;
        link_items(&items, source, target)
    } else {
        hard_link(source, target)
    }
}

/// Creates the directories and hard links the files described by `items`,
/// which must come from `read_dir_items(source)`.
///
/// `read_dir_items` walks depth-first and pushes a directory before its
/// children, so creating the directories in order is enough for the files that
/// follow to have a parent. Empty directories are in the list too, so they are
/// preserved.
///
/// Kept separate from [`link_entry`] so the filesystem side is testable without
/// an executor: `std::fs::hard_link` has no equivalent in the `Fs` trait.
fn link_items(items: &[(PathBuf, bool)], source: &Path, target: &Path) -> Result<()> {
    for (item, is_dir) in items {
        let Ok(relative) = item.strip_prefix(source) else {
            continue;
        };
        let target_item = if relative.as_os_str().is_empty() {
            target.to_path_buf()
        } else {
            target.join(relative)
        };
        if *is_dir {
            // `create_dir_all` also covers the target's own parent directories.
            std::fs::create_dir_all(&target_item)
                .with_context(|| format!("creating {target_item:?}"))?;
        } else {
            hard_link(item, &target_item)?;
        }
    }
    Ok(())
}

fn hard_link(source: &Path, target: &Path) -> Result<()> {
    std::fs::hard_link(source, target)
        .with_context(|| format!("hard linking {source:?} to {target:?}"))
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt as _;

    fn inode(path: &Path) -> u64 {
        std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("metadata {path:?}: {e}"))
            .ino()
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zed-hardlink-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ))
    }

    /// `link_items` must recreate the directory tree (empty directories
    /// included) and hard link every file, so the copy costs no disk space.
    ///
    /// The items are hand-written in the order `read_dir_items` produces
    /// (depth-first, directory before its children).
    #[test]
    fn linking_a_tree_mirrors_directories_and_shares_inodes() {
        let root = temp_root("tree");
        let source = root.join("pkg");
        std::fs::create_dir_all(source.join("sub/deep")).unwrap();
        std::fs::create_dir_all(source.join("empty")).unwrap();
        std::fs::write(source.join("a.txt"), b"hello").unwrap();
        std::fs::write(source.join("sub/b.txt"), b"world").unwrap();

        let items = vec![
            (source.clone(), true),
            (source.join("sub"), true),
            (source.join("sub/deep"), true),
            (source.join("sub/b.txt"), false),
            (source.join("empty"), true),
            (source.join("a.txt"), false),
        ];
        let target = root.join("copy");
        link_items(&items, &source, &target).unwrap();

        // Directories are recreated, empty ones included.
        assert!(target.join("sub/deep").is_dir());
        assert!(target.join("empty").is_dir());

        // Files are linked, not copied: same inode, same content.
        assert_eq!(inode(&source.join("a.txt")), inode(&target.join("a.txt")));
        assert_eq!(
            inode(&source.join("sub/b.txt")),
            inode(&target.join("sub/b.txt"))
        );
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt")).unwrap(),
            "hello"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A single file gets a second name for the same data.
    #[test]
    fn linking_a_file_shares_its_inode() {
        let root = temp_root("file");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("a.txt");
        std::fs::write(&source, b"hello").unwrap();
        let target = root.join("b.txt");

        link_items(&[(source.clone(), false)], &source, &target).unwrap();

        assert_eq!(inode(&source), inode(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");

        std::fs::remove_dir_all(&root).unwrap();
    }
}
