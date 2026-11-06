//! The location slot: the editable path entry and the breadcrumbs.
//!
//! The slot is shared by three modes (breadcrumbs / editable path / search) and
//! the text itself lives in `ui::search_field::SearchField`; this module owns
//! the state machine around it — entering, confirming and cancelling the edit,
//! plus the breadcrumb segments and their jump menu.

use std::path::{Path, PathBuf};

use gpui::{Context, DismissEvent, Focusable, Pixels, Point, SharedString, Window};
use util::ResultExt;

use super::{DeployedContextMenu, FileManagerPanel};
use crate::model::entries::{SortMode, read_entries};

impl FileManagerPanel {
    pub(crate) fn begin_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_query = None;
        let path = self.current_dir_label().to_string();
        self.path_editing = true;
        self.path_field.update(cx, |field, cx| {
            let end = path.len();
            field.set_text_with_selection(&path, 0..end, window, cx);
        });
        self.focus_path_field(window, cx);
        cx.notify();
    }

    /// Focuses the editable location entry, if it is open.
    pub(crate) fn focus_path_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_editing {
            return;
        }
        let focus = self.path_field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    pub(crate) fn cancel_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_editing {
            self.path_editing = false;
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    /// Cancels whatever inline input currently has the focus.
    ///
    /// Escape cannot be handled by the text fields directly: the default keymap
    /// binds `escape` to `menu::Cancel` with no context, and key bindings run
    /// before the fields' `on_key_down` listeners. The binding for
    /// `FileManagerPanel` therefore routes Escape here
    /// (`assets/keymaps/default-linux.json`).
    pub(crate) fn cancel_active_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_editing {
            self.cancel_path_edit(window, cx);
        } else if self.rename_state.is_some() {
            self.cancel_rename(window, cx);
        } else if self.search_query.is_some() {
            self.toggle_search(window, cx);
        }
    }

    /// Confirms whatever inline input currently has the focus, or opens the
    /// selected entry when nothing is being edited.
    ///
    /// Enter is bound to this instead of `Open`: the default keymap binds
    /// `enter` to `menu::Confirm` and key bindings are dispatched before the
    /// text fields' own key handlers, so the fields cannot confirm themselves.
    pub(crate) fn confirm_active_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_editing {
            self.confirm_path_edit(window, cx);
        } else if self.rename_state.is_some() {
            self.confirm_rename(window, cx);
        } else {
            self.open_selected(window, cx);
        }
    }

    /// Resolves the typed path and jumps to it.
    ///
    /// Supports `~`, relative paths (against the current directory) and symlink-
    /// free lexical joins. A directory is navigated into; a file selects it in
    /// its parent; a missing path keeps the editor open with the error shown.
    pub(crate) fn confirm_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_editing {
            return;
        }
        let raw = self.path_field.read(cx).content().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            // An empty location is meaningless; stay in the editor.
            self.focus_path_field(window, cx);
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
                    self.focus_path_field(window, cx);
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
                    this.path_editing = false;
                    this.selected.clear();
                    this.selection_anchor = None;
                    this.set_current_dir(target, None, window, cx);
                    window.focus(&this.focus_handle, cx);
                } else if is_file {
                    let parent = target
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| target.clone());
                    this.error = None;
                    this.path_editing = false;
                    this.selected.clear();
                    this.selected.push(target.clone());
                    this.set_current_dir(parent, None, window, cx);
                    window.focus(&this.focus_handle, cx);
                } else {
                    // Keep the editor open so the path can be fixed.
                    this.error =
                        Some(format!("No such directory or file: {}", target.display()).into());
                    this.focus_path_field(window, cx);
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

    pub(crate) fn deploy_breadcrumb_menu(
        &mut self,
        anchor: PathBuf,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.weak_entity();
        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        // The directories that share `anchor`'s parent. The root has no
        // siblings, so its own children are listed instead.
        let siblings_parent = anchor
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| anchor.clone());

        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut dirs = read_entries(
                fs,
                siblings_parent.clone(),
                show_hidden,
                SortMode::default(),
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.is_dir && entry.path != anchor)
            .collect::<Vec<_>>();
            dirs.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

            this.update_in(cx, |this, window, cx| {
                // Skip the menu when the user navigated away from `anchor`
                // while the listing was loading. `anchor` is usually an
                // *ancestor* of the current directory, so the check must be
                // "still inside it", not equality — equality is what made only
                // the last crumb open its menu.
                let still_inside = this
                    .current_dir
                    .as_deref()
                    .is_some_and(|current| current.starts_with(&anchor));
                if !still_inside {
                    return;
                }

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
                        // `dir.path` is already `<siblings_parent>/<name>`;
                        // using the name alone joined to the parent is the
                        // same path, but this keeps it exact.
                        let target = dir.path.clone();
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
}
