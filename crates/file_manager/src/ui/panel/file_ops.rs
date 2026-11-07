//! File operations: creating and deleting entries, plus the clipboard
//! (copy/cut/paste).
//!
//! Renaming lives in `panel/rename.rs` and undo/redo in `panel/history.rs`.
//! Every operation records its effect on `UndoManager` and then reloads the
//! directory, so the view and the history never disagree.

use std::path::PathBuf;

use anyhow::Result;
use fs::{CreateOptions, RemoveOptions};
use gpui::{App, Context, Entity, Focusable, PromptLevel, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::clipboard::Clipboard;
use crate::model::conflict::{TransferKind, filter_noop_sources};
use crate::model::system_clipboard;
use crate::model::undo::Change;
use crate::ui::search_field::SearchField;

impl FileManagerPanel {
    /// The inline text field that currently has the keyboard focus, if any.
    ///
    /// Copy/cut/paste/select-all are bound in the `FileManagerPanel` key context
    /// (`default-linux.json`), and key bindings run before the fields' own key
    /// handlers. The actions therefore have to forward to the focused field,
    /// otherwise Ctrl+C copies the selected *files* while the user is editing a
    /// path.
    pub(crate) fn focused_text_field(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<SearchField>> {
        for field in [&self.path_field, &self.rename_field, &self.search_field] {
            if field.read(cx).focus_handle(cx).is_focused(window) {
                return Some(field.clone());
            }
        }
        None
    }

    /// Copies either the text selection of a focused field or the selected files.
    pub(crate) fn copy_selected(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some(field) = self.focused_text_field(window, cx) {
            field.update(cx, |field, cx| field.copy_selection(cx));
            return;
        }

        let paths: Vec<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if !paths.is_empty() {
            Self::publish_to_system_clipboard(&paths, false, cx);
            self.clipboard = Some(Clipboard::Copied(paths));
            cx.notify();
        }
    }

    /// Cuts either the text selection of a focused field or the selected files.
    pub(crate) fn cut_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = self.focused_text_field(window, cx) {
            field.update(cx, |field, cx| field.cut_selection(window, cx));
            return;
        }

        let paths: Vec<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if !paths.is_empty() {
            Self::publish_to_system_clipboard(&paths, true, cx);
            self.clipboard = Some(Clipboard::Cut(paths));
            cx.notify();
        }
    }

    /// Puts `paths` on the desktop clipboard so other applications can paste
    /// them (GNOME Files reads `text/uri-list`; a cut is published as
    /// `x-special/gnome-copied-files` so it stays a move).
    ///
    /// Fire and forget: the panel's own clipboard is what its own paste uses, so
    /// a failed publish only costs interoperability.
    fn publish_to_system_clipboard(paths: &[PathBuf], cut: bool, cx: &mut Context<Self>) {
        // X11 has no `wl-copy`; skip instead of spawning something that fails.
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let paths = paths.to_vec();
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = system_clipboard::publish(&paths, cut) {
                    log::warn!("failed to publish files to the system clipboard: {error:#}");
                }
            })
            .detach();
    }

    /// Puts the selection on the clipboard in hard-link mode: pasting it creates
    /// hard links instead of duplicating the data.
    ///
    /// The regular copy/cut clipboard is untouched — this only decides what a
    /// *subsequent paste* does.
    pub(crate) fn copy_selected_as_hard_link(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if !paths.is_empty() {
            self.clipboard = Some(Clipboard::Linked(paths));
            cx.notify();
        }
    }
    fn unique_child_name(&self, base: &str) -> String {
        let mut index = 2;
        let mut candidate = base.to_string();
        while self
            .entries
            .iter()
            .any(|entry| entry.name.to_string() == candidate)
        {
            candidate = format!("{base} {index}");
            index += 1;
        }
        candidate
    }

    pub(crate) fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let path = directory.join(self.unique_child_name("New Folder"));
        let fs = self.fs.clone();
        self.spawn_operation(window, cx, path.clone(), true, async move {
            fs.create_dir(&path).await
        });
    }

    pub(crate) fn create_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_file_named("New File", window, cx);
    }

    /// Creates an empty file with the given base name.
    ///
    /// The New menu creates a file per kind (`New Markdown document.md`,
    /// `New CSV file.csv`, …) as OpenXplorer does (OPS-001, OPS-002).
    pub(crate) fn create_file_named(
        &mut self,
        base_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let path = directory.join(self.unique_child_name(base_name));
        let fs = self.fs.clone();
        self.spawn_operation(window, cx, path.clone(), false, async move {
            fs.create_file(&path, CreateOptions::default()).await
        });
    }

    /// Runs a create-type operation on `path`, records it on the undo history,
    /// refreshes the directory and starts renaming it.
    ///
    /// Zed's project panel creates the entry only when the user confirms a
    /// name (`project_panel::confirm_edit`); the panel creates it right away
    /// and puts the same name into the inline editor, so Enter keeps the
    /// suggested name and typing replaces it — the same flow for the user.
    fn spawn_operation<F>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        path: PathBuf,
        is_dir: bool,
        operation: F,
    ) where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = operation.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.error = None;
                    this.undo_manager
                        .record(vec![Change::Created(path.clone())]);
                    this.selected.clear();
                    this.selected.push(path.clone());
                    // The new entry is somewhere in the sort order, which in a
                    // long directory can be below the viewport; the load's
                    // completion scrolls it into view.
                    this.reveal_in_content(path.clone());
                    this.load_directory(window, cx);
                    // The entry is created with a suggested name and the inline
                    // editor opens on it: Enter keeps the name, typing replaces
                    // it (`project_panel::add_entry` starts in edit mode).
                    this.start_rename(path.clone(), is_dir, window, cx);
                }
                Err(error) => {
                    this.error = Some(format!("{error}").into());
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    pub(crate) fn confirm_remove(
        &mut self,
        trash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = self.selected_entries();
        if targets.is_empty() {
            return;
        };
        let count = targets.len();
        let summary = match count {
            1 => format!("“{}”", targets[0].name),
            _ => format!("{count} items"),
        };
        let message = if trash {
            format!("Move {summary} to the Trash?")
        } else {
            format!("Delete {summary} permanently?")
        };
        let confirm = if trash { "Move to Trash" } else { "Delete" };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &[confirm, "Cancel"],
            cx,
        );

        let fs = self.fs.clone();
        let paths: Vec<PathBuf> = targets.into_iter().map(|entry| entry.path).collect();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let results = futures::future::join_all(paths.iter().map(|path| async {
                if trash {
                    fs.trash(path, RemoveOptions::default())
                        .await
                        .map(|trash_id| Some(Change::Trashed(path.clone(), trash_id)))
                } else if fs.is_dir(path).await {
                    fs.remove_dir(
                        path,
                        RemoveOptions {
                            recursive: true,
                            ignore_if_not_exists: false,
                        },
                    )
                    .await
                    .map(|_| None)
                } else {
                    fs.remove_file(
                        path,
                        RemoveOptions {
                            recursive: false,
                            ignore_if_not_exists: false,
                        },
                    )
                    .await
                    .map(|_| None)
                }
            }))
            .await;
            this.update_in(cx, |this, window, cx| {
                let mut changes = Vec::new();
                this.error = None;
                for result in results {
                    match result {
                        Ok(Some(change)) => changes.push(change),
                        Ok(None) => {}
                        Err(err) => {
                            this.error = Some(format!("{err}").into());
                            break;
                        }
                    }
                }
                if this.error.is_none() {
                    this.undo_manager.record(changes);
                    this.selected.clear();
                    this.selection_anchor = None;
                    // The deleted directories must disappear from the tree.
                    for path in &paths {
                        this.prune_tree(path);
                    }
                }
                this.load_directory(window, cx);
            })
            .log_err();
        }));
    }

    pub(crate) fn paste_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Editing a path/search/name: paste the text into the field instead of
        // starting a file transfer.
        if let Some(field) = self.focused_text_field(window, cx) {
            field.update(cx, |field, cx| field.paste(window, cx));
            return;
        }

        let clipboard = self.clipboard.clone();
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        // The conflict prompt is modal: a second paste would clobber the parked
        // transfer and the answers collected so far.
        if self.pending_transfer.is_some() {
            return;
        }

        let executor = cx.background_executor().clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Files copied in another application (GNOME Files, another file
            // manager) only exist on the desktop clipboard, so it wins over the
            // panel's own copy — unless the panel holds a *hard link* clipboard,
            // which the system format cannot express.
            let linked = matches!(clipboard, Some(Clipboard::Linked(_)));
            let from_system = if linked {
                None
            } else {
                executor
                    .spawn(async move { system_clipboard::read_file_list() })
                    .await
            };

            let transfer = if let Some(list) = from_system {
                let kind = if list.cut {
                    TransferKind::Move
                } else {
                    TransferKind::Copy
                };
                Some((list.paths, kind, list.cut))
            } else {
                clipboard.map(|clipboard| {
                    let kind = match &clipboard {
                        Clipboard::Copied(_) => TransferKind::Copy,
                        Clipboard::Cut(_) => TransferKind::Move,
                        Clipboard::Linked(_) => TransferKind::Link,
                    };
                    let from_cut = matches!(clipboard, Clipboard::Cut(_));
                    (clipboard.paths().to_vec(), kind, from_cut)
                })
            };

            let Some((sources, kind, from_cut)) = transfer else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                // Skip entries that are already where they'd go: pasting a cut
                // entry back into its own directory must not rename it.
                let sources = filter_noop_sources(sources, &directory, kind);
                if sources.is_empty() {
                    return;
                }
                this.start_transfer(sources, directory, kind, from_cut, window, cx);
            })
            .ok();
        }));
    }
}
