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

use crate::model::clipboard::copy_entry;

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

/// Applies a single transfer, honouring `policy` when the target exists.
///
/// Returns the pair of paths that were actually written, or `None` when the
/// item was skipped.
pub(crate) async fn apply_transfer(
    fs: &Arc<dyn Fs>,
    source: &Path,
    target: &Path,
    copy: bool,
    policy: ConflictPolicy,
) -> Result<Option<(PathBuf, PathBuf)>> {
    let mut final_target = target.to_path_buf();

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

    if copy {
        copy_entry(fs, source, &final_target).await?;
    } else {
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

    Ok(Some((source.to_path_buf(), final_target)))
}

/// Finds a name close to `target` that does not exist yet, e.g. `notes (1).txt`.
fn unique_target(fs: &Arc<dyn Fs>, target: &Path) -> PathBuf {
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
