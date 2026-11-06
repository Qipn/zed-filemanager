//! Pinning and unpinning folders in the sidebar.
//!
//! GNOME Files toggles a bookmark for the current location with
//! <kbd>Ctrl</kbd>+<kbd>D</kbd> and offers "Star"/"Unstar" in the path bar's
//! menu (`nautilus-pathbar.c`, `nautilus-window.c`). The panel does the same
//! through [`FileManagerPanel::toggle_bookmark`], and the sidebar's rows add a
//! per-row "Remove from Bookmarks" entry.
//!
//! The list itself lives in the GTK bookmarks file, so it is shared with
//! Nautilus and the GTK file chooser (see [`crate::model::bookmarks`]).

use std::path::{Path, PathBuf};

use gpui::Context;

use super::FileManagerPanel;
use crate::model::bookmarks::{self, Bookmark};

impl FileManagerPanel {
    /// Whether `path` is pinned in the sidebar.
    pub(crate) fn is_bookmarked(&self, path: &Path) -> bool {
        self.bookmarks.iter().any(|bookmark| bookmark.path == path)
    }

    /// Whether the current directory is pinned.
    pub(crate) fn current_dir_is_bookmarked(&self) -> bool {
        self.current_dir
            .as_deref()
            .is_some_and(|directory| self.is_bookmarked(directory))
    }

    /// Pins the current directory, or unpins it when it is already pinned.
    pub(crate) fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        if self.is_bookmarked(&directory) {
            self.apply_bookmarks(bookmarks::remove(&directory), cx);
        } else {
            self.apply_bookmarks(bookmarks::add(&directory, None), cx);
        }
    }

    /// Pins a folder with the default label.
    pub(crate) fn add_bookmark(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.apply_bookmarks(bookmarks::add(&path, None), cx);
    }

    /// Unpins a folder.
    pub(crate) fn remove_bookmark(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.apply_bookmarks(bookmarks::remove(path), cx);
    }

    /// Applies the result of a bookmark-file operation: the new list becomes
    /// the sidebar's bookmark section, and a failure is reported in the panel.
    fn apply_bookmarks(&mut self, result: std::io::Result<Vec<Bookmark>>, cx: &mut Context<Self>) {
        match result {
            Ok(bookmarks) => {
                self.bookmarks = bookmarks;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("Bookmarks: {error}").into());
            }
        }
        self.rebuild_places(cx);
    }
}
