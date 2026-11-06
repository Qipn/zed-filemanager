//! Recursive search over the current folder.
//!
//! Modelled on GNOME Files (nautilus): there, a search is just another
//! `NautilusDirectory`, so the view does not special-case it — the engine emits
//! hits, the directory turns them into files, and the same model chain renders
//! them (`src/nautilus-search-directory.c`, `src/nautilus-search-engine.c`).
//!
//! This module implements the equivalent of nautilus' `model` search provider
//! (`nautilus-search-engine-model.c`): walk the folder and its subdirectories
//! and keep the entries whose name matches. Nautilus' other providers
//! (Tracker/`localsearch`, recent files, full-text) need an index or a service
//! and are out of scope for now.

use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use anyhow::Result;
use fs::Fs;
use futures::{StreamExt, future::join_all};
use gpui::SharedString;

use crate::model::entries::{DirEntry, SortMode, sort_entries};

/// How deep the walk descends below the searched folder.
///
/// Nautilus leaves this to the "search in subfolders" preference; a fixed
/// bound keeps a stray search in `/` from walking forever.
const MAX_DEPTH: usize = 8;

/// Upper bound on collected hits, so a broad query stays responsive.
pub(crate) const MAX_RESULTS: usize = 2000;

/// The outcome of one search.
pub(crate) struct SearchResults {
    /// The matching entries, sorted like the folder view.
    pub(crate) entries: Vec<DirEntry>,
    /// Whether the walk stopped early because `MAX_RESULTS` was reached.
    pub(crate) truncated: bool,
}

/// Searches `root` and its subdirectories for entries whose name contains
/// `query`, case-insensitively.
pub(crate) async fn search_directory(
    fs: Arc<dyn Fs>,
    root: PathBuf,
    query: String,
    show_hidden: bool,
    sort_mode: SortMode,
) -> Result<SearchResults> {
    let needle = query.to_lowercase();
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut found: Vec<(PathBuf, String, bool)> = Vec::new();
    let mut truncated = false;

    'walk: while let Some((directory, depth)) = queue.pop_front() {
        let mut stream = match fs.read_dir(&directory).await {
            Ok(stream) => stream,
            // An unreadable subdirectory must not abort the whole search.
            Err(_) => continue,
        };

        while let Some(path) = stream.next().await {
            let Ok(path) = path else { continue };
            let Some(name) = path.file_name() else {
                continue;
            };
            let name = name.to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            if !show_hidden && name.starts_with('.') {
                continue;
            }

            let is_dir = fs.is_dir(&path).await;

            if name.to_lowercase().contains(&needle) {
                if found.len() >= MAX_RESULTS {
                    truncated = true;
                    break 'walk;
                }
                found.push((path.clone(), name, is_dir));
            }

            if is_dir && depth + 1 <= MAX_DEPTH {
                queue.push_back((path, depth + 1));
            }
        }
    }

    let entries = join_all(found.into_iter().map(|(path, name, is_dir)| {
        // The async block must own `is_dir`, `path` and `name`, so clone the
        // `Arc` instead of borrowing it into the future.
        let fs = fs.clone();
        async move {
            let metadata = if is_dir {
                None
            } else {
                fs.metadata(&path).await.ok().flatten()
            };
            let (len, mtime) = match &metadata {
                Some(metadata) => (Some(metadata.len), Some(metadata.mtime)),
                None => (None, None),
            };
            DirEntry {
                path,
                name: SharedString::from(name),
                is_dir,
                len,
                mtime,
            }
        }
    }))
    .await;

    let mut entries: Vec<DirEntry> = entries;
    sort_entries(&mut entries, sort_mode);

    Ok(SearchResults { entries, truncated })
}
