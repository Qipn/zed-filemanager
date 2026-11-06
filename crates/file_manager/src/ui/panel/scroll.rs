//! Keeping the entry a file operation produced inside the visible part of the
//! folder view.
//!
//! A new entry lands wherever the sort order puts it, which in a long directory
//! can be below the viewport. Creating or renaming it therefore has to scroll it
//! into view, like Nautilus does after `create` / `rename`
//! (`nautilus-files-view.c` calls `scroll_to_file` on the new file).
//!
//! Two things have to happen first, and each of them is one frame away:
//!
//! 1. the directory reload that makes the row exist (`pending_reveal` holds the
//!    target across the load);
//! 2. the frame that paints the new rows — `UniformListScrollHandle` applies a
//!    pending `scroll_to_item` during prepaint of the list that has the new item
//!    count, so asking for it in the same update that assigns `entries` would let
//!    a prepaint of the *old* row count consume and clamp the request.
//!
//! `defer_scroll_content_to_path` takes care of the second step with
//! `Window::defer`, the same way the sidebar scrolls its tree
//! (`sidebar_state::defer_scroll_tree_to_current`).

use std::path::{Path, PathBuf};

use gpui::{Context, ScrollStrategy, Window};

use super::FileManagerPanel;
use crate::ui::views::ViewMode;

impl FileManagerPanel {
    /// Asks the folder view to scroll `path` into view once the directory load
    /// that contains it has landed.
    ///
    /// Call this *before* `load_directory`: the load's completion consumes it.
    pub(crate) fn reveal_in_content(&mut self, path: PathBuf) {
        self.pending_reveal = Some(path);
    }

    /// Consumes a pending reveal, called when a directory load completes.
    pub(crate) fn reveal_pending_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.pending_reveal.take() else {
            return;
        };
        self.defer_scroll_content_to_path(path, window, cx);
    }

    /// Scrolls `path` into view on the next frame.
    ///
    /// The rows the list paints were built from `entries` in the frame this is
    /// called from; the scroll itself is applied by the prepaint that follows
    /// the notify, so both the row count and the row bounds are current.
    fn defer_scroll_content_to_path(
        &self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity().downgrade();
        cx.notify();
        window.defer(cx, move |_window, cx| {
            panel
                .update(cx, |this, cx| {
                    this.scroll_content_to_path(&path);
                    cx.notify();
                })
                .ok();
        });
    }

    /// Scrolls the row of `path` into view, by the smallest amount that makes it
    /// fully visible (and not at all when it already is).
    ///
    /// The index has to be resolved against the rows the current view paints:
    /// the grid drops the children of an expanded folder, so its indices are not
    /// the details view's.
    pub(crate) fn scroll_content_to_path(&self, path: &Path) {
        let index = match self.view_mode {
            ViewMode::Details => self
                .visible_rows()
                .iter()
                .position(|(entry, _)| entry.path.as_path() == path),
            ViewMode::Grid => self
                .grid_rows()
                .iter()
                .position(|(entry, _)| entry.path.as_path() == path),
        };
        if let Some(index) = index {
            self.content_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
    }
}
