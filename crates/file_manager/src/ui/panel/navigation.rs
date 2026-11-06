//! Navigation: moving between directories, the history stack and opening
//! entries.
//!
//! All of these end up in `set_current_dir`, which reloads the folder view and
//! the folder tree. The editable path and the breadcrumbs live in
//! `panel/location.rs`, sorting in `panel/sort.rs`.

use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, TaskExt, Window};
use util::ResultExt;
use workspace::{OpenMode, OpenOptions, OpenVisible};

use super::FileManagerPanel;
use crate::model::entries::DirEntry;

impl FileManagerPanel {
    pub(crate) fn set_current_dir(
        &mut self,
        directory: PathBuf,
        selected: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.using_fallback_dir = false;
        self.current_dir = Some(directory.clone());
        self.selected.clear();
        self.selection_anchor = None;
        // Navigating leaves the search, like GNOME Files restores the location
        // it had before the search.
        self.search_query = None;
        self.clear_search_results();
        // It also closes any inline editor, so a stale path buffer or rename
        // cannot survive a jump made from the sidebar or the history buttons.
        self.path_editing = false;
        self.rename_state = None;
        if let Some(path) = selected {
            self.selected.push(path);
        }
        self.history.truncate(self.history_index + 1);
        if self.history.last() != Some(&directory) {
            self.history.push(directory);
        }
        self.history_index = self.history.len().saturating_sub(1);
        self.reload_view(window, cx);
        self.refresh_tree(window, cx);
        cx.notify();
    }

    pub(crate) fn navigate_to(
        &mut self,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_dir.as_ref() == Some(&directory) {
            // Clicking the segment of the folder we are already in would be a
            // no-op; reload instead, so the view never looks stuck.
            self.refresh(window, cx);
            return;
        }
        self.set_current_dir(directory, None, window, cx);
    }

    /// Reveals a path in this panel: selects the file, or opens the directory.
    pub fn reveal_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.is_dir() {
            self.set_current_dir(path, None, window, cx);
        } else {
            let parent = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone());
            self.set_current_dir(parent, Some(path), window, cx);
        }
    }

    /// Opens a folder as a new Zed workspace.
    ///
    /// The panel browses folders that may not belong to the current project, so
    /// this turns "found the folder here, now edit it" into one step. It goes
    /// through `Workspace::open_workspace_for_paths` with `OpenMode::NewWindow`,
    /// the same path Zed's own "Open Folder" takes (`recent_projects`), so the
    /// folder becomes the workspace root and the current window keeps whatever
    /// it had.
    ///
    /// The target is [`Self::workspace_target`]: the selected folder when there
    /// is one, otherwise the folder being browsed.
    pub(crate) fn open_as_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.workspace_target() else {
            return;
        };

        self.workspace
            .update(cx, |workspace, cx| {
                workspace.open_workspace_for_paths(OpenMode::NewWindow, vec![directory], window, cx)
            })
            .log_err()
            .map(|task| task.detach_and_log_err(cx));
    }

    /// The folder `Open as Workspace` acts on: the active selection when it is a
    /// folder, otherwise the folder being browsed.
    pub(crate) fn workspace_target(&self) -> Option<PathBuf> {
        match self.selected_entry() {
            Some(entry) if entry.is_dir => Some(entry.path),
            _ => self.current_dir.clone(),
        }
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.history_index -= 1;
        self.current_dir = self.history.get(self.history_index).cloned();
        self.selected.clear();
        self.selection_anchor = None;
        self.using_fallback_dir = false;
        // Leaving the search, like Windows Explorer's back button restores the
        // location it had before the search.
        self.search_query = None;
        self.clear_search_results();
        self.reload_view(window, cx);
        cx.notify();
    }

    pub(crate) fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        self.history_index += 1;
        self.current_dir = self.history.get(self.history_index).cloned();
        self.selected.clear();
        self.selection_anchor = None;
        self.using_fallback_dir = false;
        self.search_query = None;
        self.clear_search_results();
        self.reload_view(window, cx);
        cx.notify();
    }

    pub(crate) fn go_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // While searching, "up" leaves the search and returns to the folder the
        // search started from, like Windows Explorer returns to the scope.
        if self.search_query.is_some() {
            self.toggle_search(window, cx);
            return;
        }
        let Some(parent) = self
            .current_dir
            .as_ref()
            .and_then(|directory| directory.parent())
            .map(Path::to_path_buf)
        else {
            return;
        };
        self.set_current_dir(parent, None, window, cx);
    }

    /// Refreshes whatever the view currently shows: the folder, or the hits of
    /// an active search.
    pub(crate) fn reload_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_searching() {
            self.start_search(window, cx);
        } else {
            self.load_directory(window, cx);
        }
    }

    pub(crate) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_view(window, cx);
    }

    /// Toggles hidden-entry visibility and reloads.
    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let options = OpenOptions {
            visible: Some(OpenVisible::None),
            ..Default::default()
        };
        let task = workspace.update(cx, |workspace, cx| {
            workspace.open_abs_path(path, options, window, cx)
        });
        task.detach_and_log_err(cx);
    }

    pub(crate) fn open_entry(
        &mut self,
        entry: DirEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.is_dir {
            self.navigate_to(entry.path, window, cx);
        } else {
            self.open_file(entry.path, window, cx);
        }
    }
    pub(crate) fn current_dir_label(&self) -> SharedString {
        self.current_dir
            .as_ref()
            .map(|directory| directory.display().to_string().into())
            .unwrap_or_else(|| "No folder open".into())
    }
}
