//! The panel's clipboard state and file-operation helpers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use fs::{CopyOptions, Fs};

/// The panel's internal clipboard for copy/cut/paste. Holds every selected
/// path so multi-selection can be pasted as a batch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Clipboard {
    Copied(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
}

impl Clipboard {
    /// The paths this clipboard entry refers to.
    pub(crate) fn paths(&self) -> &[PathBuf] {
        match self {
            Clipboard::Copied(paths) | Clipboard::Cut(paths) => paths,
        }
    }
}

/// Active inline rename: the entry being renamed and the typed buffer.
#[derive(Debug)]
pub(crate) struct RenameState {
    pub(crate) source: PathBuf,
    pub(crate) buffer: String,
}

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
