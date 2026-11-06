//! The search mode of the location slot and the recursive search behind it.
//!
//! The panel keeps the query, the hits and a generation counter; superseded
//! results are dropped when they arrive, which mirrors nautilus cancelling its
//! engine through a `GCancellable` (`nautilus-search-provider.c`).

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;

impl FileManagerPanel {
    /// Opens or closes the search mode.
    ///
    /// The location slot is shared with the path bar (like GNOME Files), so
    /// entering the search leaves the editable path.
    pub(crate) fn toggle_search(&mut self, cx: &mut Context<Self>) {
        match self.search_query.take() {
            Some(_) => {
                self.clear_search_results();
            }
            None => {
                self.path_edit = None;
                self.search_query = Some(String::new());
                self.clear_search_results();
            }
        }
        cx.notify();
    }

    /// Whether a non-empty search query is active.
    pub(crate) fn is_searching(&self) -> bool {
        self.search_query
            .as_ref()
            .is_some_and(|query| !query.is_empty())
    }

    /// Drops the hits of the previous search and marks nothing as running.
    pub(crate) fn clear_search_results(&mut self) {
        self.search_results = None;
        self.search_running = false;
        self.search_generation = self.search_generation.wrapping_add(1);
    }

    /// Starts (or restarts) the recursive search for the current query.
    ///
    /// Every keystroke supersedes the previous run; results of a superseded
    /// generation are discarded when they arrive, which is the equivalent of
    /// nautilus cancelling its engine with a `GCancellable`
    /// (`nautilus-search-provider.c`).
    pub(crate) fn start_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;

        let Some(query) = self.search_query.clone().filter(|query| !query.is_empty()) else {
            self.search_results = None;
            self.search_running = false;
            cx.notify();
            return;
        };

        let Some(root) = self.current_dir.clone() else {
            return;
        };

        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let sort_mode = self.sort_mode;

        self.search_running = true;
        self.selected.clear();
        self.selection_anchor = None;
        self._search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result =
                crate::model::search::search_directory(fs, root, query, show_hidden, sort_mode)
                    .await;
            this.update(cx, |this, cx| {
                if this.search_generation != generation {
                    return;
                }
                this.search_running = false;
                match result {
                    Ok(results) => {
                        this.search_results = Some(results);
                        this.error = None;
                    }
                    Err(error) => {
                        this.error = Some(format!("{error}").into());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }
    /// Appends a keystroke to the search query and restarts the search.
    pub(crate) fn search_input(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = self.search_query.as_mut() {
            query.push_str(key);
        }
        self.start_search(window, cx);
    }

    /// Removes the last character of the search query.
    pub(crate) fn search_backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = self.search_query.as_mut() {
            query.pop();
        }
        self.start_search(window, cx);
    }
}
