//! Transfers: dropping entries onto a directory and resolving name conflicts.
//!
//! Paste, cut and drag & drop all funnel into `start_transfer`, which first
//! plans the targets, then either runs them or parks the transfer in
//! `pending_transfer` until the user picks a conflict policy.

use std::path::PathBuf;

use gpui::{Context, Window};
use util::ResultExt;

use super::{DraggedPaths, FileManagerPanel, PendingTransfer};
use crate::model::conflict::{ConflictPolicy, apply_transfer, conflicting_indices, plan_targets};
use crate::model::undo::Change;

impl FileManagerPanel {
    pub(crate) fn drop_dragged(
        &mut self,
        dragged: DraggedPaths,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_target = None;
        // Dropping a selection onto itself (or any selected path) falls back
        // to the current directory.
        let destination = if self.selected.contains(&target) {
            self.current_dir.clone()
        } else {
            Some(target)
        };
        let Some(destination) = destination else {
            return;
        };
        let sources: Vec<PathBuf> = dragged
            .0
            .into_iter()
            .filter(|source| *source != destination)
            .collect();
        if sources.is_empty() {
            return;
        }
        // Holding Ctrl copies the dragged entries instead of moving them.
        let copy = window.modifiers().control;
        self.start_transfer(sources, destination, copy, false, window, cx);
    }

    /// External drop: copies the dragged files into `target`.
    pub(crate) fn drop_external(
        &mut self,
        paths: Vec<PathBuf>,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_target = None;
        // External files are always copied, never moved.
        self.start_transfer(paths, target, true, false, window, cx);
    }

    /// Starts a transfer of `sources` into `destination`.
    ///
    /// Name conflicts are detected first; when any exist the transfer is
    /// suspended and the user is asked how to resolve them (see
    /// [`Self::resolve_conflicts`]).
    pub(crate) fn start_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        copy: bool,
        from_cut: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        let targets = plan_targets(&sources, &destination);
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let conflicts = conflicting_indices(&fs, &targets);
            this.update_in(cx, |this, window, cx| {
                if conflicts.is_empty() {
                    let policies = vec![ConflictPolicy::KeepBoth; targets.len()];
                    this.execute_transfer(sources, targets, copy, from_cut, policies, window, cx);
                } else {
                    this.pending_transfer = Some(PendingTransfer {
                        sources,
                        targets,
                        copy,
                        conflicts,
                        from_cut,
                    });
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    /// Applies the user's conflict resolution to the suspended transfer.
    pub(crate) fn resolve_conflicts(
        &mut self,
        policy: ConflictPolicy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_transfer.take() else {
            return;
        };
        let policies: Vec<ConflictPolicy> = pending
            .targets
            .iter()
            .enumerate()
            .map(|(index, _)| {
                if pending.conflicts.contains(&index) {
                    policy
                } else {
                    ConflictPolicy::KeepBoth
                }
            })
            .collect();
        self.execute_transfer(
            pending.sources,
            pending.targets,
            pending.copy,
            pending.from_cut,
            policies,
            window,
            cx,
        );
    }

    /// Aborts the suspended transfer, leaving the filesystem untouched.
    pub(crate) fn cancel_conflicts(&mut self, cx: &mut Context<Self>) {
        self.pending_transfer = None;
        cx.notify();
    }

    /// Performs the transfer itself, applying `policies` per item.
    fn execute_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
        copy: bool,
        from_cut: bool,
        policies: Vec<ConflictPolicy>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut changes: Vec<Change> = Vec::new();
            let mut written: Vec<PathBuf> = Vec::new();
            let mut error = None;

            for (index, (source, target)) in sources.iter().zip(targets.iter()).enumerate() {
                let policy = policies
                    .get(index)
                    .copied()
                    .unwrap_or(ConflictPolicy::KeepBoth);
                match apply_transfer(&fs, source, target, copy, policy).await {
                    Ok(Some((source, final_target))) => {
                        if copy {
                            changes.push(Change::Created(final_target.clone()));
                        } else {
                            changes.push(Change::Renamed(source, final_target.clone()));
                        }
                        written.push(final_target);
                    }
                    // The item was skipped because of a conflict.
                    Ok(None) => {}
                    Err(err) => {
                        error = Some(format!("{err}"));
                        break;
                    }
                }
            }

            this.update_in(cx, |this, window, cx| {
                this.error = error.map(Into::into);
                if this.error.is_none() {
                    this.undo_manager.record(changes);
                    if from_cut {
                        this.clipboard = None;
                    }
                    this.selected.clear();
                    this.selected.extend(written);
                    this.load_directory(window, cx);
                } else {
                    cx.notify();
                }
            })
            .log_err();
        }));
    }
}
