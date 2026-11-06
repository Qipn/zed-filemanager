//! File operations: creating entries, renaming, deleting and the undo history
//! hooks that wrap them.
//!
//! Every operation records its effect on `UndoManager` and then reloads the
//! directory, so the view and the history never disagree.

use std::path::PathBuf;

use anyhow::Result;
use fs::{CreateOptions, RemoveOptions, RenameOptions};
use gpui::{App, Context, Entity, PromptLevel, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::clipboard::{Clipboard, RenameState};
use crate::model::undo::{Change, HistoryStep};
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
            self.clipboard = Some(Clipboard::Cut(paths));
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

    pub(crate) fn undo_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_history_step(window, cx, HistoryStep::Undo);
    }

    pub(crate) fn redo_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_history_step(window, cx, HistoryStep::Redo);
    }

    /// Executes the inverse of the newest change on the given stack, then
    /// records the resulting state on the opposite stack.
    fn run_history_step(&mut self, window: &mut Window, cx: &mut Context<Self>, step: HistoryStep) {
        let can_run = match step {
            HistoryStep::Undo => self.undo_manager.can_undo(),
            HistoryStep::Redo => self.undo_manager.can_redo(),
        };
        if !can_run {
            return;
        }
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let popped = this
                .update(cx, |this, _cx| match step {
                    HistoryStep::Undo => this.undo_manager.pop_undo(),
                    HistoryStep::Redo => this.undo_manager.pop_redo(),
                })
                .ok()
                .flatten();
            let Some(change) = popped else {
                return;
            };

            let result = crate::model::undo::execute_operation(&*fs, change.to_inverse()).await;

            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(new_change) => {
                        this.undo_manager.commit_inverse(step, new_change);
                        this.error = None;
                    }
                    Err(error) => {
                        this.undo_manager.restore_step(step, change);
                        this.error = Some(format!("{error}").into());
                    }
                }
                this.load_directory(window, cx);
            })
            .log_err();
        }));
    }
    pub(crate) fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        self.start_rename(entry.path, entry.is_dir, window, cx);
    }

    /// Starts the inline rename of `path`.
    ///
    /// Like `project_panel::rename_impl`, the field starts with the current
    /// name selected — the whole name for a directory, only the stem for a file
    /// ("Only files keep their extension unselected for quick renames").
    pub(crate) fn start_rename(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
        else {
            return;
        };
        let selection = if is_dir {
            0..name.len()
        } else {
            let stem_len = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().len())
                .unwrap_or(name.len());
            0..stem_len
        };

        self.rename_state = Some(RenameState { source: path });
        self.rename_field.update(cx, |field, cx| {
            field.set_text_with_selection(&name, selection, window, cx);
        });
        self.focus_rename_field(window, cx);
        cx.notify();
    }

    /// Focuses the inline rename editor, if a rename is running.
    pub(crate) fn focus_rename_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename_state.is_none() {
            return;
        }
        let focus = self.rename_field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Commits the inline rename.
    ///
    /// The checks follow `project_panel::confirm_edit` / `populate_validation_error`:
    /// an empty name, a path separator, leading or trailing whitespace and an
    /// existing target are refused — and the editor stays open so the user can
    /// fix the name, instead of failing silently.
    pub(crate) fn confirm_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.rename_state.as_ref().map(|state| state.source.clone()) else {
            return;
        };
        let Some(directory) = source.parent().map(|parent| parent.to_path_buf()) else {
            return;
        };

        // `project_panel::confirm_edit` strips trailing dots on Windows, where
        // the file system ignores them.
        let mut new_name = self.rename_field.read(cx).content().to_string();
        if cfg!(windows) {
            while let Some(stripped) = new_name.strip_suffix('.') {
                new_name = stripped.to_string();
            }
        }

        if new_name.is_empty() {
            self.error = Some("A file or folder name cannot be empty.".into());
            cx.notify();
            return;
        }
        if new_name.contains('/') || new_name.contains('\0') {
            self.error = Some("A file or folder name cannot contain “/”.".into());
            cx.notify();
            return;
        }
        if new_name.trim() != new_name {
            self.error = Some("A file or folder name cannot start or end with a space.".into());
            cx.notify();
            return;
        }

        let target = directory.join(&new_name);
        if target == source {
            // Renaming to the same name is a no-op.
            self.rename_state = None;
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        if target.exists() {
            self.error = Some(format!("“{new_name}” already exists here.").into());
            cx.notify();
            return;
        }

        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = fs
                .rename(
                    &source,
                    &target,
                    RenameOptions {
                        overwrite: false,
                        // Fail instead of silently doing nothing when another
                        // process created the target in the meantime.
                        ignore_if_exists: false,
                        create_parents: false,
                    },
                )
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.error = None;
                    this.rename_state = None;
                    this.selected.clear();
                    this.selected.push(target.clone());
                    this.load_directory(window, cx);
                    window.focus(&this.focus_handle, cx);
                }
                Err(error) => {
                    // Keep the editor open so the name can be fixed.
                    this.error = Some(format!("{error}").into());
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rename_state.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    pub(crate) fn paste_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Editing a path/search/name: paste the text into the field instead of
        // starting a file transfer.
        if let Some(field) = self.focused_text_field(window, cx) {
            field.update(cx, |field, cx| field.paste(window, cx));
            return;
        }

        let Some(clipboard) = self.clipboard.clone() else {
            return;
        };
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let sources: Vec<PathBuf> = clipboard
            .paths()
            .iter()
            .filter(|source| {
                source
                    .file_name()
                    .is_some_and(|name| directory.join(name) != **source)
            })
            .cloned()
            .collect();
        if sources.is_empty() {
            return;
        }
        // A cut clipboard moves the entries, a copied one duplicates them.
        let copy = matches!(clipboard, Clipboard::Copied(_));
        let from_cut = matches!(clipboard, Clipboard::Cut(_));
        self.start_transfer(sources, directory, copy, from_cut, window, cx);
    }
}
