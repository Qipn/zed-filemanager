//! Sorting and hidden-file visibility.
//!
//! Every change reloads the current view, so the list and the status bar
//! (`Sorted by …`) always agree.

use gpui::{Context, Window};

use super::FileManagerPanel;
use crate::model::entries::SortMode;

impl FileManagerPanel {
    pub(crate) fn toggle_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.persist_state(cx);
        self.reload_view(window, cx);
        cx.notify();
    }

    /// Sorts by name, toggling the direction when the column is already active.
    pub(crate) fn sort_by_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_name() {
            self.sort_mode.toggled()
        } else {
            SortMode::NameAscending
        };
        self.persist_state(cx);
        self.reload_view(window, cx);
        cx.notify();
    }

    /// Sorts by modification date.
    pub(crate) fn sort_by_modified(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_modified() {
            self.sort_mode.toggled()
        } else {
            SortMode::ModifiedAscending
        };
        self.persist_state(cx);
        self.reload_view(window, cx);
        cx.notify();
    }

    /// Sorts by size.
    pub(crate) fn sort_by_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_size() {
            self.sort_mode.toggled()
        } else {
            SortMode::SizeAscending
        };
        self.persist_state(cx);
        self.reload_view(window, cx);
        cx.notify();
    }
}
