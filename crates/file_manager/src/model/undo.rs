//! Panel-scoped undo/redo for file operations.
//!
//! A simplified version of `project_panel`'s `UndoManager`: our operations are
//! plain absolute-path `Fs` calls, so there is no actor loop, no
//! `ProjectPath`/worktree bookkeeping and no collab restrictions.
//!
//! The manager uses two stacks of performed changes:
//!
//! - `undo`: everything done so far, newest last.
//! - `redo`: undone changes, newest last.
//!
//! Undo pops from `undo`, executes the inverse and pushes the resulting change
//! onto `redo`; redo pops from `redo`, executes the inverse and pushes onto
//! `undo`. Recording a new change clears `redo`.

use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::Result;
use fs::{Fs, RemoveOptions, RenameOptions, TrashId};

/// Maximum number of recorded changes per stack.
const MAX_UNDO_CHANGES: usize = 10_000;

/// An undoable filesystem operation. Executing it produces a [`Change`]
/// describing what is now in effect.
#[derive(Debug)]
pub(crate) enum Operation {
    /// Moves a path to the system trash.
    Trash(PathBuf),
    /// Restores a previously trashed item to its original location.
    Restore(TrashId),
    /// Moves a path to a new location (`rename`).
    Rename(PathBuf, PathBuf),
    /// Executes the operations as one step, in order.
    ///
    /// Inverses execute in reverse order so that dependencies hold: undoing
    /// "create `dir/` then `dir/main.rs`" removes the file first.
    Batch(Vec<Operation>),
}

/// What actually happened on disk. Inverting it yields the [`Operation`]
/// that reverses the change.
#[derive(Clone, Debug)]
pub(crate) enum Change {
    /// The item exists at this path (created, or restored from the trash).
    Created(PathBuf),
    /// The item was moved to the system trash; `trash_id` identifies the
    /// trashed copy for later restoration.
    #[allow(dead_code)]
    Trashed(PathBuf, TrashId),
    /// The item was moved from `from` to `to`.
    Renamed(PathBuf, PathBuf),
    /// Several changes performed as one step.
    Batched(Vec<Change>),
}

impl Change {
    /// The operation that reverses this change.
    pub(crate) fn to_inverse(&self) -> Operation {
        match self {
            Change::Created(path) => Operation::Trash(path.clone()),
            Change::Trashed(_, trash_id) => Operation::Restore(*trash_id),
            Change::Renamed(from, to) => Operation::Rename(to.clone(), from.clone()),
            // Reversed so dependencies between the changes hold (see
            // [`Operation::Batch`]).
            Change::Batched(changes) => {
                Operation::Batch(changes.iter().rev().map(Change::to_inverse).collect())
            }
        }
    }
}

/// Executes `operation` through `fs`, returning the change that describes the
/// resulting state. Trash operations do not prompt again: everything on the
/// history was confirmed when originally performed.
pub(crate) async fn execute_operation(fs: &dyn Fs, operation: Operation) -> Result<Change> {
    match operation {
        Operation::Trash(path) => {
            let trash_id = fs.trash(&path, RemoveOptions::default()).await?;
            Ok(Change::Trashed(path, trash_id))
        }
        Operation::Restore(trash_id) => {
            let path = fs.restore(trash_id).await?;
            Ok(Change::Created(path))
        }
        Operation::Rename(from, to) => {
            fs.rename(
                &from,
                &to,
                RenameOptions {
                    overwrite: false,
                    ignore_if_exists: true,
                    create_parents: false,
                },
            )
            .await?;
            Ok(Change::Renamed(from, to))
        }
        Operation::Batch(operations) => {
            let mut changes = Vec::new();
            for operation in operations {
                // Box the future to avoid a recursively-unsized future.
                changes.push(Box::pin(execute_operation(fs, operation)).await?);
            }
            match changes.len() {
                0 => Ok(Change::Batched(changes)),
                1 => Ok(changes.into_iter().next().expect("one change")),
                _ => Ok(Change::Batched(changes)),
            }
        }
    }
}

/// Which stack a history step operates on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HistoryStep {
    Undo,
    Redo,
}

/// Two-stack history of performed changes.
pub(crate) struct UndoManager {
    undo: VecDeque<Change>,
    redo: VecDeque<Change>,
}

impl UndoManager {
    pub(crate) fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Records performed changes as a single step and clears the redo stack.
    pub(crate) fn record(&mut self, changes: Vec<Change>) {
        match changes.len() {
            0 => return,
            1 => self
                .undo
                .push_back(changes.into_iter().next().expect("one change")),
            _ => self.undo.push_back(Change::Batched(changes)),
        }
        self.redo.clear();
        while self.undo.len() > MAX_UNDO_CHANGES {
            self.undo.pop_front();
        }
    }

    /// Removes the newest change from the undo stack so its inverse can be
    /// executed. On failure the caller must [`Self::restore_step`] it back.
    pub(crate) fn pop_undo(&mut self) -> Option<Change> {
        self.undo.pop_back()
    }

    /// Removes the newest change from the redo stack so its inverse can be
    /// executed. On failure the caller must [`Self::restore_step`] it back.
    pub(crate) fn pop_redo(&mut self) -> Option<Change> {
        self.redo.pop_back()
    }

    /// Puts a popped change back onto the stack it came from, used when
    /// executing its inverse failed and nothing changed on disk.
    pub(crate) fn restore_step(&mut self, step: HistoryStep, change: Change) {
        match step {
            HistoryStep::Undo => self.undo.push_back(change),
            HistoryStep::Redo => self.redo.push_back(change),
        }
    }

    /// Records the change produced by executing an inverse: it lands on the
    /// opposite stack so redo (or a second undo) can reverse it again.
    pub(crate) fn commit_inverse(&mut self, step: HistoryStep, change: Change) {
        match step {
            HistoryStep::Undo => self.redo.push_back(change),
            HistoryStep::Redo => self.undo.push_back(change),
        }
    }
}
