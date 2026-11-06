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

/// Active inline rename: the entry being renamed.
///
/// The typed text lives in the panel's [`crate::ui::search_field::SearchField`]
/// entity, so the inline editor gets a real caret, selection and input-method
/// support (the rename row renders that entity in place of the name).
#[derive(Debug)]
pub(crate) struct RenameState {
    /// The entry that is being renamed.
    pub(crate) source: PathBuf,
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
