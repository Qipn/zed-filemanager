//! The folder tree shown in the sidebar.
//!
//! OpenXplorer keeps the tree below the "Places" list in the sidebar
//! (`src/window/folder_tree.rs` in the upstream repository). The tree is lazy:
//! a directory's children are only read when it is expanded, and they are
//! dropped again when it is collapsed.
//!
//! Only directories are listed; files never appear in the tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs::Fs;
use futures::StreamExt;
use gpui::SharedString;

/// One node of the folder tree.
#[derive(Clone, Debug)]
pub(crate) struct TreeNode {
    /// The directory this node points at.
    pub(crate) path: PathBuf,
    /// The display name.
    pub(crate) name: SharedString,
    /// Nesting level, `0` for the direct children of the root.
    pub(crate) depth: usize,
    /// Whether the node's children are currently shown.
    pub(crate) expanded: bool,
    /// Whether the children have been read at least once.
    pub(crate) loaded: bool,
    /// The child directories.
    pub(crate) children: Vec<TreeNode>,
}

impl TreeNode {
    /// Creates a node for `path`.
    pub(crate) fn new(path: PathBuf, name: SharedString, depth: usize) -> Self {
        Self {
            path,
            name,
            depth,
            expanded: false,
            loaded: false,
            children: Vec::new(),
        }
    }

    /// Finds a node by path, at any depth, mutably.
    pub(crate) fn find_mut<'a>(nodes: &'a mut [TreeNode], path: &Path) -> Option<&'a mut TreeNode> {
        for node in nodes {
            if node.path == path {
                return Some(node);
            }
            if let Some(found) = Self::find_mut(&mut node.children, path) {
                return Some(found);
            }
        }
        None
    }

    /// Removes the node at `path`, if present.
    pub(crate) fn remove(nodes: &mut Vec<TreeNode>, path: &Path) -> bool {
        let Some(index) = nodes.iter().position(|node| node.path == path) else {
            return false;
        };
        nodes.remove(index);
        true
    }
}

/// Reads the sub-directories of `directory`, sorted by name.
///
/// Hidden directories are skipped, matching the "Show hidden files" setting of
/// the main view.
pub(crate) async fn read_directories(
    fs: &Arc<dyn Fs>,
    directory: &Path,
    depth: usize,
    show_hidden: bool,
) -> Vec<TreeNode> {
    let Ok(mut stream) = fs.read_dir(directory).await else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    while let Some(entry) = stream.next().await {
        let Ok(path) = entry else { continue };
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.to_string_lossy().to_string();
        if name.is_empty() || (!show_hidden && name.starts_with('.')) {
            continue;
        }
        paths.push((path, name));
    }

    let mut directories = Vec::new();
    for (path, name) in paths {
        if fs.is_dir(&path).await {
            directories.push(TreeNode::new(path, name.into(), depth));
        }
    }

    directories.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    directories
}

/// Flattens the tree into the rows that are currently visible.
///
/// A node contributes a row, and its children contribute rows as well when it
/// is expanded.
pub(crate) fn visible_rows(nodes: &[TreeNode]) -> Vec<&TreeNode> {
    let mut rows = Vec::new();
    collect_rows(nodes, &mut rows);
    rows
}

fn collect_rows<'a>(nodes: &'a [TreeNode], rows: &mut Vec<&'a TreeNode>) {
    for node in nodes {
        rows.push(node);
        if node.expanded {
            collect_rows(&node.children, rows);
        }
    }
}

/// The chain of ancestors of `path`, from the root down to the directory itself.
///
/// Used to auto-expand the tree down to the directory that is currently open.
pub(crate) fn ancestor_chain(path: &Path, root: &Path) -> Vec<PathBuf> {
    let mut chain = Vec::new();
    let Ok(relative) = path.strip_prefix(root) else {
        return chain;
    };
    let mut current = root.to_path_buf();
    chain.push(current.clone());
    for component in relative.components() {
        current.push(component);
        chain.push(current.clone());
    }
    chain
}
