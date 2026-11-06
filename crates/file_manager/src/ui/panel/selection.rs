//! The visible rows of the folder view and the selection they support.
//!
//! Everything that indexes rows (clicking, keyboard navigation, the details
//! pane) goes through `visible_rows`, so the flat list of search hits and the
//! expanded folder tree behave the same way.

use std::path::{Path, PathBuf};

use gpui::{Context, KeyDownEvent, Window};

use super::FileManagerPanel;
use crate::model::entries::DirEntry;

impl FileManagerPanel {
    /// The rows shown in the folder view: the entries of the current
    /// directory, with every expanded directory followed by its children.
    ///
    /// While a search is active the rows are the search hits instead, as a flat
    /// list: Nautilus does not expand folders while searching either
    /// (`nautilus-name-cell.c` hides the expander when `search_directory` is
    /// set).
    ///
    /// The second element of the pair is the nesting depth, used for the
    /// indentation of the tree.
    pub(crate) fn visible_rows(&self) -> Vec<(DirEntry, usize)> {
        if self.is_searching() {
            return self
                .search_results
                .as_ref()
                .map(|results| {
                    results
                        .entries
                        .iter()
                        .cloned()
                        .map(|entry| (entry, 0))
                        .collect()
                })
                .unwrap_or_default();
        }

        let query = self
            .search_query
            .as_ref()
            .map(|query| query.to_lowercase())
            .filter(|query| !query.is_empty());
        let matches = |name: &str| match &query {
            Some(query) => name.to_lowercase().contains(query),
            None => true,
        };

        let mut rows = Vec::new();
        for entry in &self.entries {
            if !matches(&entry.name) {
                continue;
            }
            rows.push((entry.clone(), 0));
            if entry.is_dir {
                if let Some(children) = self.expanded_dirs.get(&entry.path) {
                    push_child_rows(children, 1, &matches, &self.expanded_dirs, &mut rows);
                }
            }
        }
        rows
    }

    /// The "active" selected entry (the one rename operates on).
    pub(crate) fn selected_entry(&self) -> Option<DirEntry> {
        let selected = self.selected.last()?;
        self.visible_rows()
            .into_iter()
            .find(|(entry, _)| &entry.path == selected)
            .map(|(entry, _)| entry)
    }

    /// Every selected entry, in directory order.
    pub(crate) fn selected_entries(&self) -> Vec<DirEntry> {
        self.visible_rows()
            .into_iter()
            .filter(|(entry, _)| self.selected.contains(&entry.path))
            .map(|(entry, _)| entry)
            .collect()
    }

    pub(crate) fn is_selected(&self, path: &Path) -> bool {
        self.selected.iter().any(|selected| selected == path)
    }

    pub(crate) fn select_only(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        if let Some((entry, _)) = rows.get(index) {
            self.selected.clear();
            self.selected.push(entry.path.clone());
            self.selection_anchor = Some(index);
            cx.notify();
        }
    }

    /// Selects every visible row, like GNOME Files' "Select All" in the
    /// background context menu (`Ctrl+A`).
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        self.selected = rows.iter().map(|(entry, _)| entry.path.clone()).collect();
        self.selection_anchor = (!rows.is_empty()).then_some(0);
        cx.notify();
    }

    /// Clears the selection, used when the background of the view is clicked.
    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        self.selected.clear();
        self.selection_anchor = None;
        cx.notify();
    }

    pub(crate) fn toggle_select(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        let Some((entry, _)) = rows.get(index) else {
            return;
        };
        if let Some(position) = self
            .selected
            .iter()
            .position(|selected| selected == &entry.path)
        {
            self.selected.remove(position);
        } else {
            self.selected.push(entry.path.clone());
            self.selection_anchor = Some(index);
        }
        cx.notify();
    }

    pub(crate) fn select_range(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        let anchor = self.selection_anchor.or_else(|| {
            self.selected
                .last()
                .and_then(|path| rows.iter().position(|(entry, _)| &entry.path == path))
        });
        let Some(anchor) = anchor else {
            self.select_only(index, cx);
            return;
        };
        self.selected.clear();
        for index in anchor.min(index)..=anchor.max(index) {
            if let Some((entry, _)) = rows.get(index) {
                self.selected.push(entry.path.clone());
            }
        }
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.last()?;
        self.visible_rows()
            .iter()
            .position(|(entry, _)| &entry.path == selected)
    }

    fn select_relative(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let row_count = self.visible_rows().len();
        if row_count == 0 {
            return;
        }
        let Some(current) = self.selected_index() else {
            let first = if delta < 0 { row_count - 1 } else { 0 };
            self.select_only(first, cx);
            return;
        };
        let next = if delta < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(row_count - 1)
        };
        if extend {
            self.select_range(next, cx);
        } else {
            self.select_only(next, cx);
        }
    }

    pub(crate) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_entry() {
            self.open_entry(entry, window, cx);
        }
    }

    pub(crate) fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        // While the search entry is open it owns the keystrokes; the panel only
        // keeps Escape so the mode can still be left when the focus sits on the
        // list (after clicking a hit).
        if self.search_query.is_some() {
            if event.keystroke.key == "escape" {
                self.toggle_search(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        // While the location entry is open it owns the keys (it is focused and
        // stops propagation); the panel only keeps Escape, so the mode can still
        // be left when the focus sits on the list.
        if self.path_edit.is_some() {
            if event.keystroke.key == "escape" {
                self.cancel_path_edit(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        // While an inline rename is active the rename field owns the keys (it
        // is focused, and stops propagation), so nothing is handled here.
        if self.rename_state.is_some() {
            return;
        }

        match event.keystroke.key.as_str() {
            "up" => {
                self.select_relative(-1, modifiers.shift, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.select_relative(1, modifiers.shift, cx);
                cx.stop_propagation();
            }
            "home" => {
                self.select_only(0, cx);
                cx.stop_propagation();
            }
            "end" => {
                if !self.entries.is_empty() {
                    self.select_only(self.entries.len() - 1, cx);
                }
                cx.stop_propagation();
            }
            "backspace" => {
                self.go_up(window, cx);
                cx.stop_propagation();
            }
            "f5" => {
                self.refresh(window, cx);
                cx.stop_propagation();
            }
            "delete" => {
                if modifiers.shift {
                    self.confirm_remove(false, window, cx);
                } else {
                    self.confirm_remove(true, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

/// Appends the children of an expanded directory to the visible rows.
fn push_child_rows(
    children: &[DirEntry],
    depth: usize,
    matches: &impl Fn(&str) -> bool,
    expanded: &std::collections::HashMap<PathBuf, Vec<DirEntry>>,
    rows: &mut Vec<(DirEntry, usize)>,
) {
    for child in children {
        if !matches(&child.name) {
            continue;
        }
        rows.push((child.clone(), depth));
        if child.is_dir {
            if let Some(grandchildren) = expanded.get(&child.path) {
                push_child_rows(grandchildren, depth + 1, matches, expanded, rows);
            }
        }
    }
}
