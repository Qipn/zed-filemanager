//! Batch rename: one pattern applied to the whole selection.
//!
//! The pattern uses `#` for the running number (`photo-#` → `photo-1`,
//! `photo-2`, …) and keeps the original extension when it has none of its own.
//! That is the template mode of Nautilus' batch renamer
//! (`nautilus-batch-rename-dialog.c`); find/replace is not part of it yet.

use gpui::{Context, Focusable, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::undo::Change;

impl FileManagerPanel {
    /// Opens the prompt, prefilled with the extension of the first selected file.
    pub(crate) fn begin_batch_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.selected_entries();
        if entries.len() < 2 {
            return;
        }
        let extension = entries
            .first()
            .and_then(|entry| entry.path.extension())
            .map(|extension| format!(".{}", extension.to_string_lossy()))
            .unwrap_or_default();
        let pattern = format!("#{extension}");

        self.batch_rename_open = true;
        self.batch_field.update(cx, |field, cx| {
            let end = pattern.len();
            field.set_text_with_selection(&pattern, 0..end, window, cx);
            field.focus_handle(cx).focus(window, cx);
        });
        cx.notify();
    }

    /// Closes the prompt without touching the files.
    pub(crate) fn cancel_batch_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.batch_rename_open {
            return;
        }
        self.batch_rename_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Renames the selection with the typed pattern.
    pub(crate) fn confirm_batch_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.batch_rename_open {
            return;
        }
        let pattern = self.batch_field.read(cx).content().trim().to_string();
        let entries = self.selected_entries();
        if pattern.is_empty() || !pattern.contains('#') || entries.len() < 2 {
            // Stay open so the pattern can be fixed.
            self.error = Some("Use `#` for the number, e.g. photo-#".into());
            cx.notify();
            return;
        }

        self.batch_rename_open = false;
        self.error = None;
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut changes: Vec<Change> = Vec::new();
            let mut written = Vec::new();
            let mut error = None;

            for (index, entry) in entries.iter().enumerate() {
                let mut name = pattern.replace('#', &(index + 1).to_string());
                // A pattern without an extension keeps the original one.
                if std::path::Path::new(&name).extension().is_none()
                    && let Some(extension) = entry.path.extension()
                {
                    name.push('.');
                    name.push_str(&extension.to_string_lossy());
                }

                let Some(parent) = entry.path.parent() else {
                    continue;
                };
                let target = parent.join(&name);
                if target == entry.path {
                    continue;
                }
                // Refuse rather than overwrite: the user is renaming, not
                // replacing.
                if fs.path_exists(&target) {
                    error = Some(format!("“{name}” already exists"));
                    break;
                }
                if let Err(err) = fs
                    .rename(
                        &entry.path,
                        &target,
                        fs::RenameOptions {
                            overwrite: false,
                            ignore_if_exists: false,
                            create_parents: false,
                        },
                    )
                    .await
                {
                    error = Some(format!("{err}"));
                    break;
                }
                changes.push(Change::Renamed(entry.path.clone(), target.clone()));
                written.push(target);
            }

            this.update_in(cx, |this, window, cx| {
                this.error = error.map(Into::into);
                if !changes.is_empty() {
                    this.undo_manager
                        .record(vec![Change::Batched(changes.clone())]);
                }
                if this.error.is_none() {
                    this.selected.clear();
                    this.selected.extend(written);
                }
                this.load_directory(window, cx);
            })
            .log_err();
        }));
    }
}
