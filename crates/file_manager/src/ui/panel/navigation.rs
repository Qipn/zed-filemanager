//! Navigation: moving between directories, the history stack, sorting, the
//! editable path bar and opening entries.
//!
//! All of these end up in `set_current_dir`, which reloads the folder view and
//! the folder tree.

use std::path::{Path, PathBuf};

use gpui::{Context, DismissEvent, Focusable, Pixels, Point, SharedString, TaskExt, Window};
use util::ResultExt;
use workspace::{OpenOptions, OpenVisible};

use super::{DeployedContextMenu, FileManagerPanel};
use crate::model::entries::{DirEntry, SortMode, read_entries};

impl FileManagerPanel {
    fn set_current_dir(
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
            self.toggle_search(cx);
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
    pub(crate) fn toggle_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
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
        self.reload_view(window, cx);
        cx.notify();
    }

    /// Enters path-bar editing with the current directory prefilled.
    ///
    /// The location slot is shared with the search (like GNOME Files), so
    /// opening the editable path leaves the search mode.
    pub(crate) fn begin_path_edit(&mut self, cx: &mut Context<Self>) {
        self.search_query = None;
        self.path_edit = Some(self.current_dir_label().to_string());
        cx.notify();
    }

    pub(crate) fn cancel_path_edit(&mut self, cx: &mut Context<Self>) {
        self.path_edit = None;
        cx.notify();
    }

    pub(crate) fn path_edit_input(&mut self, key: &str, modifiers: gpui::Modifiers) {
        let Some(buffer) = self.path_edit.as_mut() else {
            return;
        };
        let _ = modifiers;
        match key {
            "space" => buffer.push(' '),
            key if key.len() == 1 => buffer.push_str(key),
            _ => {}
        }
    }

    pub(crate) fn path_edit_backspace(&mut self) {
        if let Some(buffer) = self.path_edit.as_mut() {
            buffer.pop();
        }
    }

    /// Resolves the typed path and jumps to it.
    ///
    /// Supports `~`, relative paths (against the current directory) and symlink-
    /// free lexical joins. A directory is navigated into; a file selects it in
    /// its parent; a missing path keeps the editor open with the error shown.
    pub(crate) fn confirm_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(raw) = self.path_edit.take() else {
            return;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            cx.notify();
            return;
        }

        let mut expanded: PathBuf = if let Some(stripped) = trimmed.strip_prefix("~") {
            util::paths::home_dir().join(stripped.trim_start_matches('/'))
        } else {
            trimmed.into()
        };
        if expanded.is_relative() {
            match &self.current_dir {
                Some(directory) => expanded = directory.join(expanded),
                None => {
                    self.error = Some("No folder open; enter an absolute path".into());
                    self.path_edit = Some(raw);
                    cx.notify();
                    return;
                }
            }
        }
        let target = util::paths::normalize_path(&expanded);

        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let is_dir = fs.is_dir(&target).await;
            let is_file = !is_dir && fs.is_file(&target).await;
            this.update_in(cx, |this, window, cx| {
                if is_dir {
                    this.error = None;
                    this.selected.clear();
                    this.selection_anchor = None;
                    this.set_current_dir(target, None, window, cx);
                } else if is_file {
                    let parent = target
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| target.clone());
                    this.error = None;
                    this.selected.clear();
                    this.selected.push(target.clone());
                    this.set_current_dir(parent, None, window, cx);
                } else {
                    this.error =
                        Some(format!("No such directory or file: {}", target.display()).into());
                    this.path_edit = Some(raw);
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Jump target for a breadcrumb segment.
    pub(crate) fn breadcrumb_targets(&self) -> Vec<(PathBuf, SharedString)> {
        let Some(directory) = &self.current_dir else {
            return Vec::new();
        };
        let mut segments = Vec::new();
        let mut accumulator = PathBuf::new();
        for component in directory.components() {
            use std::path::Component;
            match component {
                Component::RootDir => {
                    accumulator.push(std::path::MAIN_SEPARATOR.to_string());
                    segments.push((accumulator.clone(), "/".into()));
                }
                Component::Prefix(prefix) => {
                    accumulator.push(prefix.as_os_str());
                    segments.push((
                        accumulator.clone(),
                        prefix.as_os_str().to_string_lossy().into(),
                    ));
                }
                component => {
                    accumulator.push(component.as_os_str());
                    segments.push((
                        accumulator.clone(),
                        component.as_os_str().to_string_lossy().into(),
                    ));
                }
            }
        }
        segments
    }

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
    /// Builds and deploys a breadcrumb dropdown for `anchor`: the first entry
    /// navigates to `anchor` itself, then every directory sibling (i.e. child
    /// of `anchor`'s parent) is listed for a lateral switch. Called for a
    /// *click* on a breadcrumb segment.
    pub(crate) fn deploy_breadcrumb_menu(
        &mut self,
        anchor: PathBuf,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.weak_entity();
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut dirs = read_entries(fs, anchor.clone(), true, SortMode::default())
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| entry.is_dir)
                .collect::<Vec<_>>();
            dirs.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

            this.update_in(cx, |this, window, cx| {
                if this.current_dir.as_deref() != Some(anchor.as_path()) {
                    // Navigated elsewhere while loading; skip the menu.
                    return;
                }
                let siblings_parent = anchor
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| anchor.clone());
                let focus = this.focus_handle.clone();
                let menu = ui::ContextMenu::build(window, cx, |menu, _, _| {
                    let mut menu = menu
                        .context(focus)
                        .entry("跳转到此目录", None, {
                            let panel = panel.clone();
                            let anchor = anchor.clone();
                            move |window, cx| {
                                panel
                                    .update(cx, |state, cx| {
                                        state.navigate_to(anchor.clone(), window, cx);
                                    })
                                    .ok();
                            }
                        })
                        .separator();
                    for dir in dirs {
                        let target = siblings_parent
                            .join(dir.path.file_name().expect("read_dir yields file names"));
                        menu = menu.entry(dir.name.clone(), None, {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel
                                    .update(cx, |state, cx| {
                                        state.navigate_to(target.clone(), window, cx);
                                    })
                                    .ok();
                            }
                        });
                    }
                    menu
                });
                window.focus(&menu.focus_handle(cx), cx);
                let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
                    this.context_menu.take();
                    cx.notify();
                });
                this.context_menu = Some(DeployedContextMenu {
                    menu,
                    position,
                    _subscription: subscription,
                });
                cx.notify();
            })
            .log_err();
        }));
    }
    pub(crate) fn current_dir_label(&self) -> SharedString {
        self.current_dir
            .as_ref()
            .map(|directory| directory.display().to_string().into())
            .unwrap_or_else(|| "No folder open".into())
    }
}
