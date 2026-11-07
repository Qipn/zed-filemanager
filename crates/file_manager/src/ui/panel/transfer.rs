//! Transfers: dropping entries onto a directory and resolving name conflicts.
//!
//! Paste, cut and drag & drop all funnel into `start_transfer`, which first
//! plans the targets, then either runs them or parks the transfer in
//! `pending_transfer` until the user picks a conflict policy.

use std::path::PathBuf;

use gpui::{Context, Focusable, SharedString, Window};
use util::ResultExt;

use super::{ConflictDecision, DraggedPaths, FileManagerPanel, PendingTransfer};
use crate::model::conflict::{
    ConflictPolicy, TransferKind, apply_transfer, conflicting_indices, filter_noop_sources,
    plan_targets, unique_target,
};
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
        // Holding Ctrl copies the dragged entries instead of moving them.
        let kind = if window.modifiers().control {
            TransferKind::Copy
        } else {
            TransferKind::Move
        };
        // Dropping an entry onto its own row (or into its own directory) must be a
        // no-op: the planned target is the entry itself, and answering the conflict
        // prompt with "Replace" would delete it before renaming it onto itself.
        let sources = filter_noop_sources(dragged.0, &destination, kind);
        if sources.is_empty() {
            return;
        }
        self.start_transfer(sources, destination, kind, false, window, cx);
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
        self.start_transfer(paths, target, TransferKind::Copy, false, window, cx);
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
        kind: TransferKind,
        from_cut: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        // The conflict prompt is modal: without this a second paste would clobber
        // the parked transfer and its answers.
        if self.pending_transfer.is_some() {
            return;
        }
        let targets = plan_targets(&sources, &destination);
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let conflicts = conflicting_indices(&fs, &targets);
            this.update_in(cx, |this, window, cx| {
                if conflicts.is_empty() {
                    let policies = vec![ConflictPolicy::KeepBoth; targets.len()];
                    let item_count = sources.len();
                    this.execute_transfer(
                        sources,
                        targets,
                        kind,
                        from_cut,
                        policies,
                        vec![None; item_count],
                        window,
                        cx,
                    );
                } else {
                    let decisions = vec![None; conflicts.len()];
                    this.pending_transfer = Some(PendingTransfer {
                        sources,
                        targets,
                        kind,
                        conflicts,
                        from_cut,
                        current: 0,
                        decisions,
                    });
                    this.conflict_apply_to_all = false;
                    this.fill_conflict_field(window, cx);
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    /// Fills the conflict prompt with a usable name for the conflict being shown
    /// and focuses it, so the user can type a different one right away.
    fn fill_conflict_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_transfer.as_ref() else {
            return;
        };
        let Some(index) = pending.conflicts.get(pending.current).copied() else {
            return;
        };
        let Some(target) = pending.targets.get(index).cloned() else {
            return;
        };
        // The suggestion keeps `name (1).ext` so pressing the name button is a
        // one-click "keep both".
        let suggestion = unique_target(&self.fs, &target);
        let name = suggestion
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();

        self.conflict_field.update(cx, |field, cx| {
            field.set_text_with_selection(&name, 0..name.len(), window, cx);
            field.focus_handle(cx).focus(window, cx);
        });
    }

    /// Answers the conflict being shown.
    ///
    /// With `use_typed_name` the name in the prompt wins over the planned target
    /// (that is the "rename" answer); otherwise `policy` decides what happens to
    /// the existing entry. With the apply-to-all switch on, the same answer is
    /// given to every remaining conflict.
    pub(crate) fn resolve_current_conflict(
        &mut self,
        policy: ConflictPolicy,
        use_typed_name: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(position) = self
            .pending_transfer
            .as_ref()
            .map(|pending| pending.current)
        else {
            return;
        };
        // Read the prompt's text before taking a mutable borrow of the panel.
        let typed = self.conflict_field.read(cx).content().trim().to_string();
        let apply_to_all = self.conflict_apply_to_all;

        let Some(pending) = self.pending_transfer.as_mut() else {
            return;
        };
        if position >= pending.conflicts.len() {
            return;
        }

        let decision_target = if use_typed_name && !typed.is_empty() {
            pending
                .targets
                .get(pending.conflicts[position])
                .and_then(|target| target.parent())
                .map(|parent| parent.join(&typed))
        } else {
            None
        };

        let decision = ConflictDecision {
            // A typed name replaces the target, so the existing entry behind the
            // planned name is left alone.
            policy: if decision_target.is_some() {
                ConflictPolicy::KeepBoth
            } else {
                policy
            },
            target: decision_target,
        };

        pending.decisions[position] = Some(decision);
        if apply_to_all {
            for entry in pending.decisions.iter_mut().skip(position + 1) {
                if entry.is_none() {
                    // A typed name can only apply to the item it was typed for.
                    *entry = Some(ConflictDecision {
                        policy,
                        target: None,
                    });
                }
            }
        }

        let next = pending.decisions.iter().position(|entry| entry.is_none());
        if let Some(next) = next {
            pending.current = next;
        }

        match next {
            Some(_) => {
                self.fill_conflict_field(window, cx);
                cx.notify();
            }
            None => self.finish_conflicts(window, cx),
        }
    }

    /// Runs the suspended transfer with the answers collected so far.
    fn finish_conflicts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_transfer.take() else {
            return;
        };
        self.conflict_apply_to_all = false;
        let mut policies = vec![ConflictPolicy::KeepBoth; pending.targets.len()];
        let mut chosen = vec![None; pending.targets.len()];
        for (position, index) in pending.conflicts.iter().enumerate() {
            let decision = pending
                .decisions
                .get(position)
                .and_then(|entry| entry.as_ref());
            if let Some(decision) = decision {
                policies[*index] = decision.policy;
                chosen[*index] = decision.target.clone();
            }
        }
        self.execute_transfer(
            pending.sources,
            pending.targets,
            pending.kind,
            pending.from_cut,
            policies,
            chosen,
            window,
            cx,
        );
    }

    /// Aborts the suspended transfer, leaving the filesystem untouched.
    pub(crate) fn cancel_conflicts(&mut self, cx: &mut Context<Self>) {
        self.pending_transfer = None;
        self.conflict_apply_to_all = false;
        cx.notify();
    }

    /// Performs the transfer itself, applying `policies` per item.
    #[allow(clippy::too_many_arguments)]
    fn execute_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
        kind: TransferKind,
        from_cut: bool,
        policies: Vec<ConflictPolicy>,
        chosen_targets: Vec<Option<PathBuf>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fs = self.fs.clone();
        // The progress row polls this flag; setting it makes the loop stop after
        // the entry it is working on, so a cancelled transfer leaves whole
        // entries behind (which the undo history records as usual).
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.transfer_progress = Some(super::TransferProgress {
            total: sources.len(),
            done: 0,
            current: SharedString::default(),
            kind,
            cancelled: cancelled.clone(),
        });
        cx.notify();

        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut changes: Vec<Change> = Vec::new();
            let mut written: Vec<PathBuf> = Vec::new();
            let mut error = None;

            for (index, (source, target)) in sources.iter().zip(targets.iter()).enumerate() {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let name: SharedString = source
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .into();
                this.update(cx, |this, cx| {
                    if let Some(progress) = this.transfer_progress.as_mut() {
                        progress.current = name;
                    }
                    cx.notify();
                })
                .ok();

                let policy = policies
                    .get(index)
                    .copied()
                    .unwrap_or(ConflictPolicy::KeepBoth);
                let chosen = chosen_targets
                    .get(index)
                    .and_then(|target| target.as_deref());
                match apply_transfer(&fs, source, target, kind, policy, chosen).await {
                    Ok(Some((source, final_target))) => {
                        if kind == TransferKind::Move {
                            changes.push(Change::Renamed(source, final_target.clone()));
                        } else {
                            // Copy and hard link both leave the source in place and
                            // create a new entry at the target.
                            changes.push(Change::Created(final_target.clone()));
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

                this.update(cx, |this, cx| {
                    if let Some(progress) = this.transfer_progress.as_mut() {
                        progress.done = index + 1;
                    }
                    cx.notify();
                })
                .ok();
            }

            this.update_in(cx, |this, window, cx| {
                this.transfer_progress = None;
                this.error = error.map(Into::into);
                if this.error.is_none() {
                    this.undo_manager.record(changes);
                    if from_cut {
                        this.clipboard = None;
                        // The source moved, so the desktop selection must not
                        // offer the same paths again (GNOME Files clears its own
                        // selection for the same reason).
                        cx.background_executor()
                            .spawn(async move {
                                if let Err(error) = crate::model::system_clipboard::clear() {
                                    log::warn!("failed to clear the system clipboard: {error:#}");
                                }
                            })
                            .detach();
                    }
                    this.selected.clear();
                    this.selected.extend(written);
                    // Pasted/dropped entries can land below the viewport; the
                    // last one is the one kept selected.
                    if let Some(path) = this.selected.last().cloned() {
                        this.reveal_in_content(path);
                    }
                    this.load_directory(window, cx);
                } else {
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    /// Stops the running transfer after the entry it is working on.
    pub(crate) fn cancel_transfer(&mut self, cx: &mut Context<Self>) {
        if let Some(progress) = self.transfer_progress.as_ref() {
            progress
                .cancelled
                .store(true, std::sync::atomic::Ordering::Relaxed);
            cx.notify();
        }
    }
}
