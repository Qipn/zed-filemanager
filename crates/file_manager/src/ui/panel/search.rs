//! The search mode of the location slot and the recursive search behind it.
//!
//! The panel keeps the query, the hits and a generation counter; superseded
//! results are dropped when they arrive, which mirrors nautilus cancelling its
//! engine through a `GCancellable` (`nautilus-search-provider.c`).

use gpui::{Context, Focusable, Window};
use util::ResultExt;

use super::FileManagerPanel;

impl FileManagerPanel {
    /// Opens or closes the search mode.
    ///
    /// The location slot is shared with the path bar (like GNOME Files), so
    /// entering the search leaves the editable path. Opening the mode focuses
    /// the entry, exactly like GNOME's search ToggleButton swaps the toolbar to
    /// the search page and grabs the focus.
    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.search_query.take() {
            Some(_) => {
                self.clear_search_results();
                self.search_field
                    .update(cx, |field, cx| field.set_text("", window, cx));
                window.focus(&self.focus_handle, cx);
            }
            None => {
                self.path_editing = false;
                // Only one inline editor may hold the focus, like
                // `project_panel` discarding its edit state on a new action.
                if self.rename_state.is_some() {
                    self.cancel_rename(window, cx);
                }
                self.search_query = Some(String::new());
                self.search_field
                    .update(cx, |field, cx| field.set_text("", window, cx));
                let focus = self.search_field.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
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

    /// Starts the search after a short delay.
    ///
    /// GNOME Files coalesces the keystrokes of one burst into an idle callback
    /// (`nautilus-query-editor.c::entry_changed_cb` uses `g_idle_add_once`), so
    /// typing fast does not walk the tree once per character. The delay also
    /// makes a pending run cancellable by [`Self::clear_search_results`], which
    /// bumps the same generation counter.
    const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

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
        let options = self.search_options.clone();

        self.search_running = true;
        self.selected.clear();
        self.selection_anchor = None;
        self._search_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Coalesce a burst of keystrokes instead of walking the tree once
            // per character.
            cx.background_executor().timer(Self::SEARCH_DEBOUNCE).await;
            if !matches!(
                this.read_with(cx, |this, _| this.search_generation == generation),
                Ok(true)
            ) {
                return;
            }
            let result = crate::model::search::search_directory(
                fs,
                root,
                query,
                show_hidden,
                sort_mode,
                &options,
            )
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
}
