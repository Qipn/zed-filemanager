//! Undo/redo: replaying the panel's own history (`model::undo`).
//!
//! Every operation records its effect on `UndoManager`; this module turns a
//! user's Undo/Redo into the inverse operation and keeps both stacks coherent
//! when an operation fails.

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::undo::HistoryStep;

impl FileManagerPanel {
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
}
