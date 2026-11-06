//! Inline rename: starting the editor, validating the new name and committing it.
//!
//! The typed text lives in the panel's `SearchField` entity, so renaming gets a
//! real caret, selection and input-method support; this module only owns the
//! state machine and the checks copied from Zed's project panel
//! (`project_panel::confirm_edit` / `populate_validation_error`).

use std::path::PathBuf;

use fs::RenameOptions;
use gpui::{Context, Focusable, Window};
use util::ResultExt;

use super::FileManagerPanel;

impl FileManagerPanel {
    pub(crate) fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        self.start_rename(entry.path, entry.is_dir, window, cx);
    }

    /// Whether `path` is the entry currently being renamed.
    ///
    /// The details and grid views render the inline editor in place of the
    /// name when this is true.
    pub(crate) fn is_renaming(&self, path: &std::path::Path) -> bool {
        self.rename_state.as_deref() == Some(path)
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

        self.rename_state = Some(path);
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
        let Some(source) = self.rename_state.clone() else {
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
                    // Renaming can move the entry to another place in the sort
                    // order, so the row has to be found again.
                    this.reveal_in_content(target.clone());
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
}
