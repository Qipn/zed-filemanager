//! The sidebar state: the "Places" list and the folder tree.

use std::path::Path;

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::tree::{self, TreeNode};
use crate::ui::places::{build_places, home_dir};

impl FileManagerPanel {
    // ---- OpenXplorer layout: places, folder tree, sidebar and view mode ----

    /// Rebuilds the "Places" list of the sidebar.
    pub(crate) fn refresh_places(&mut self, cx: &mut Context<Self>) {
        let home = home_dir();
        let roots = self
            .project
            .read(cx)
            .worktree_paths(cx)
            .folder_path_list()
            .paths()
            .to_vec();
        self.places = build_places(home.as_ref(), roots);
    }

    /// Makes sure the folder tree has a root and shows its children.
    ///
    /// OpenXplorer roots the tree at the home directory when the current
    /// folder lives inside it, and at the current folder otherwise.
    pub(crate) fn refresh_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = home_dir();
        let current = self.current_dir.clone();
        let root = match (&home, &current) {
            (Some(home), Some(current)) if current.starts_with(home) => home.clone(),
            (_, Some(current)) => current.clone(),
            (Some(home), None) => home.clone(),
            (None, None) => return,
        };

        if self.tree_root.as_ref() == Some(&root) {
            self.sync_tree_expansion();
            return;
        }

        self.tree_root = Some(root.clone());
        self.tree_nodes.clear();
        self.load_tree_children(&root, 0, window, cx);
    }

    /// Reads the sub-directories of `directory` into the tree.
    fn load_tree_children(
        &mut self,
        directory: &Path,
        depth: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let directory = directory.to_path_buf();
        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let children = tree::read_directories(&fs, &directory, depth + 1, show_hidden).await;
            this.update(cx, |this, cx| {
                if this.tree_root.as_deref() == Some(directory.as_path()) {
                    this.tree_nodes = children;
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Expands or collapses a node of the folder tree.
    pub(crate) fn toggle_tree_node(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = TreeNode::find_mut(&mut self.tree_nodes, path) else {
            return;
        };
        if node.expanded {
            node.expanded = false;
            node.children.clear();
            node.loaded = false;
            cx.notify();
            return;
        }
        if node.loaded {
            node.expanded = true;
            cx.notify();
            return;
        }

        // The children are read the first time the node is opened.
        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let depth = node.depth;
        let path = path.to_path_buf();
        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let children = tree::read_directories(&fs, &path, depth + 1, show_hidden).await;
            this.update(cx, |this, cx| {
                if let Some(node) = TreeNode::find_mut(&mut this.tree_nodes, &path) {
                    node.children = children;
                    node.loaded = true;
                    node.expanded = true;
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Expands the already loaded ancestors of the current directory.
    fn sync_tree_expansion(&mut self) {
        let Some(root) = self.tree_root.clone() else {
            return;
        };
        let Some(current) = self.current_dir.clone() else {
            return;
        };
        let chain = tree::ancestor_chain(&current, &root);
        for path in chain.iter().skip(1) {
            if let Some(node) = TreeNode::find_mut(&mut self.tree_nodes, path) {
                node.expanded = true;
            }
        }
    }

    /// Removes a directory from the tree, e.g. after it was deleted.
    pub(crate) fn prune_tree(&mut self, path: &Path) {
        TreeNode::remove(&mut self.tree_nodes, path);
    }
}
