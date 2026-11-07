//! Conflict resolution for transfers (paste, cut, drag & drop).
//!
//! A transfer moves or copies a set of source paths into a destination
//! directory. When a destination entry with the same name already exists the
//! user decides what happens: replace it, skip it, or keep both by generating
//! a unique name such as `notes (1).txt`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use fs::{Fs, RemoveOptions, RenameOptions};

use crate::model::clipboard::{copy_entry, link_entry};

/// How a transfer writes its targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferKind {
    /// `rename` the source into place.
    Move,
    /// Duplicate the bytes (`copy_file` / `copy_recursive`).
    Copy,
    /// Create hard links instead of duplicating anything: no extra disk, but the
    /// source and the new name share an inode, so writing through either updates
    /// both.
    Link,
}

/// What to do when the destination already contains an entry with the same
/// name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConflictPolicy {
    /// Delete the existing entry and write the new one in its place.
    Replace,
    /// Leave the existing entry alone and do not transfer this item.
    Skip,
    /// Keep the existing entry and write the new one under a unique name.
    KeepBoth,
}

impl ConflictPolicy {
    /// The label shown on the corresponding button in the conflict prompt.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ConflictPolicy::Replace => "Replace",
            ConflictPolicy::Skip => "Skip",
            ConflictPolicy::KeepBoth => "Keep both",
        }
    }
}

/// Computes the destination path of every source inside `directory`.
pub(crate) fn plan_targets(sources: &[PathBuf], directory: &Path) -> Vec<PathBuf> {
    sources
        .iter()
        .filter_map(|source| source.file_name().map(|name| directory.join(name)))
        .collect()
}

/// Returns the indices of targets that already exist on disk.
pub(crate) fn conflicting_indices(fs: &Arc<dyn Fs>, targets: &[PathBuf]) -> Vec<usize> {
    targets
        .iter()
        .enumerate()
        .filter(|(_, target)| fs.path_exists(target))
        .map(|(index, _)| index)
        .collect()
}

/// Drops the sources whose transfer into `directory` must not happen at all.
///
/// Two cases:
///
/// - **The target of a source is the source itself** (`directory` is the source
///   or sits inside it). Letting this reach the conflict prompt is destructive:
///   `Replace` removes the entry and then renames it onto itself (data loss),
///   and `Keep both` renames the folder to `name (1)`. Dropping an entry onto
///   its own row lands here.
/// - **A move into the directory the source already lives in.** Nothing to do
///   either; without this it degenerates into the same conflict path and the
///   entry gets renamed to `name (1)` just for being dropped where it already
///   is.
///
/// Copies keep both cases out of the second rule: a Ctrl-drag or copy+paste
/// into the same directory is a legitimate "duplicate this here".
pub(crate) fn filter_noop_sources(
    sources: Vec<PathBuf>,
    directory: &Path,
    kind: TransferKind,
) -> Vec<PathBuf> {
    sources
        .into_iter()
        .filter(|source| {
            if directory.starts_with(source) {
                return false;
            }
            !(kind == TransferKind::Move && source.parent() == Some(directory))
        })
        .collect()
}

/// Applies a single transfer, honouring `policy` when the target exists.
///
/// `chosen_target` is the name the user typed in the conflict prompt; it replaces
/// the planned target (and then goes through the same conflict handling as any
/// other target, so typing a name that already exists cannot silently overwrite
/// it).
///
/// Returns the pair of paths that were actually written, or `None` when the
/// item was skipped.
pub(crate) async fn apply_transfer(
    fs: &Arc<dyn Fs>,
    source: &Path,
    target: &Path,
    kind: TransferKind,
    policy: ConflictPolicy,
    chosen_target: Option<&Path>,
) -> Result<Option<(PathBuf, PathBuf)>> {
    let mut final_target = chosen_target.unwrap_or(target).to_path_buf();

    if fs.path_exists(&final_target) {
        match policy {
            ConflictPolicy::Skip => return Ok(None),
            ConflictPolicy::Replace => {
                if fs.is_dir(&final_target).await {
                    fs.remove_dir(
                        &final_target,
                        RemoveOptions {
                            recursive: true,
                            ignore_if_not_exists: true,
                        },
                    )
                    .await?;
                } else {
                    fs.remove_file(
                        &final_target,
                        RemoveOptions {
                            recursive: false,
                            ignore_if_not_exists: true,
                        },
                    )
                    .await?;
                }
            }
            ConflictPolicy::KeepBoth => {
                final_target = unique_target(fs, target);
            }
        }
    }

    match kind {
        TransferKind::Copy => copy_entry(fs, source, &final_target).await?,
        TransferKind::Link => link_entry(fs, source, &final_target).await?,
        TransferKind::Move => {
            fs.rename(
                source,
                &final_target,
                RenameOptions {
                    overwrite: false,
                    ignore_if_exists: false,
                    create_parents: false,
                },
            )
            .await?;
        }
    }

    Ok(Some((source.to_path_buf(), final_target)))
}

/// Finds a name close to `target` that does not exist yet, e.g. `notes (1).txt`.
///
/// Also used to prefill the conflict prompt with a usable suggestion.
pub(crate) fn unique_target(fs: &Arc<dyn Fs>, target: &Path) -> PathBuf {
    let directory = target.parent().unwrap_or_else(|| Path::new(""));
    let stem = target.file_stem().unwrap_or_default().to_os_string();
    let extension = target.extension().map(|ext| ext.to_os_string());

    for index in 1..10_000 {
        let mut name = OsString::from(&stem);
        name.push(format!(" ({index})"));
        if let Some(extension) = &extension {
            name.push(".");
            name.push(extension);
        }
        let candidate = directory.join(name);
        if !fs.path_exists(&candidate) {
            return candidate;
        }
    }

    // Extremely unlikely; fall back to a timestamped name.
    let mut fallback = OsString::from(&stem);
    fallback.push(format!(
        " ({})",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    ));
    if let Some(extension) = &extension {
        fallback.push(".");
        fallback.push(extension);
    }
    directory.join(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn dropping_onto_its_own_directory_is_a_noop_when_moving() {
        // The target of `A` would be `A` itself: `Keep both` would rename it to
        // `A (1)` and `Replace` would delete it before renaming onto itself.
        let sources = paths(&["/home/alice/A"]);
        assert!(
            filter_noop_sources(sources, Path::new("/home/alice"), TransferKind::Move).is_empty(),
            "a move into the directory the entry already lives in does nothing"
        );
    }

    #[test]
    fn dropping_onto_its_own_directory_still_copies() {
        // Ctrl-drag / copy+paste in place is a real operation.
        let sources = paths(&["/home/alice/A"]);
        assert_eq!(
            filter_noop_sources(sources, Path::new("/home/alice"), TransferKind::Copy),
            paths(&["/home/alice/A"])
        );
    }

    #[test]
    fn linking_in_place_is_allowed_like_copying() {
        let sources = paths(&["/home/alice/A"]);
        assert_eq!(
            filter_noop_sources(sources, Path::new("/home/alice"), TransferKind::Link),
            paths(&["/home/alice/A"]),
            "a hard link into the same directory produces a second name, not a rename"
        );
    }

    #[test]
    fn dropping_an_entry_onto_itself_is_a_noop() {
        let sources = paths(&["/home/alice/A"]);
        assert!(
            filter_noop_sources(sources, Path::new("/home/alice/A"), TransferKind::Copy).is_empty()
        );
    }

    #[test]
    fn dropping_an_entry_into_its_own_subtree_is_a_noop() {
        let sources = paths(&["/home/alice/A"]);
        assert!(
            filter_noop_sources(
                paths(&["/home/alice/A"]),
                Path::new("/home/alice/A/sub"),
                TransferKind::Move
            )
            .is_empty()
        );
        // ...but a sibling that merely shares a prefix is not "inside".
        assert_eq!(
            filter_noop_sources(sources, Path::new("/home/alice/A (1)"), TransferKind::Move),
            paths(&["/home/alice/A"])
        );
    }

    #[test]
    fn moves_between_directories_are_kept() {
        assert_eq!(
            filter_noop_sources(
                paths(&["/home/alice/A"]),
                Path::new("/home/alice/b"),
                TransferKind::Move
            ),
            paths(&["/home/alice/A"])
        );
    }

    #[test]
    fn only_the_entries_already_in_place_are_dropped() {
        let sources = paths(&["/home/alice/A", "/tmp/B", "/home/alice/C"]);
        assert_eq!(
            filter_noop_sources(sources, Path::new("/home/alice"), TransferKind::Move),
            paths(&["/tmp/B"]),
            "A and C are already in /home/alice, B comes from elsewhere"
        );
    }
}
