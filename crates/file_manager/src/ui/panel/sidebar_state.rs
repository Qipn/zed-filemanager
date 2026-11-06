//! The sidebar state: the "Places" list and the folder tree.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::tree::{self, TreeNode};
use crate::model::xdg::home_dir;
use crate::ui::ox_theme::metrics;
use crate::ui::places::build_places;

impl FileManagerPanel {
    // ---- OpenXplorer layout: places, folder tree, sidebar and view mode ----

    /// Rebuilds the "Places" list of the sidebar, re-reading the bookmarks.
    pub(crate) fn refresh_places(&mut self, cx: &mut Context<Self>) {
        self.bookmarks = crate::model::bookmarks::load();
        self.rebuild_places(cx);
    }

    /// Rebuilds the "Places" list from the cached bookmarks.
    pub(crate) fn rebuild_places(&mut self, cx: &mut Context<Self>) {
        let home = home_dir();
        let roots = self
            .project
            .read(cx)
            .worktree_paths(cx)
            .folder_path_list()
            .paths()
            .to_vec();
        self.places = build_places(home.as_ref(), roots, &self.bookmarks);
        cx.notify();
    }

    /// Makes sure the folder tree has a root and shows its children.
    ///
    /// The root is **sticky**: it only changes when the current folder leaves
    /// it. Starting at `/`, browsing anywhere on the filesystem keeps the tree
    /// rooted at `/`; starting inside the home directory keeps it rooted there
    /// until the user jumps outside.
    ///
    /// This matters because re-rooting rebuilds the tree: navigating to
    /// `/home/qipn` while the tree was rooted at `/` used to switch the root to
    /// the home directory, which wiped the expanded nodes and made the click
    /// that caused it look like it had been swallowed ("clicking `qipn` needs
    /// two clicks").
    pub(crate) fn refresh_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let home = home_dir();
        let current = self.current_dir.clone();

        // Keep the existing root while the current folder is still inside it.
        if let (Some(root), Some(current)) = (&self.tree_root, &current) {
            if current.starts_with(root) {
                self.sync_tree_expansion();
                return;
            }
        }

        let root = match (&home, &current) {
            (Some(home), Some(current)) if current.starts_with(home) => home.clone(),
            // Outside the home directory the tree spans the whole filesystem.
            (_, Some(_)) => PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            (Some(home), None) => home.clone(),
            (None, None) => return,
        };

        if self.tree_root.as_ref() == Some(&root) {
            self.sync_tree_expansion();
            self.reveal_in_tree(window, cx);
            return;
        }

        self.tree_root = Some(root.clone());
        self.tree_nodes.clear();
        // A rebuilt tree must be revealed again from scratch.
        self.last_revealed = None;
        self.load_tree_children(&root, 0, window, cx);
    }

    /// Expands the tree down to the current directory and scrolls it into view.
    ///
    /// This is the "scroll to reveal" every file manager does: jumping to a deep
    /// path (breadcrumb, sidebar place, search hit, history) is useless if the
    /// tree still shows a collapsed branch. The walk is lazy — every level is
    /// read on demand, one directory at a time — and it only ever *expands*:
    /// branches the user collapsed elsewhere are left alone, and a manual
    /// collapse of an ancestor is respected until the current directory changes
    /// again (`last_revealed`).
    pub(crate) fn reveal_in_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.tree_root.clone() else {
            return;
        };
        let Some(current) = self.current_dir.clone() else {
            return;
        };
        if self.last_revealed.as_ref() == Some(&current) {
            return;
        }

        // `chain` is `[root, …, current]`; the current directory itself is not
        // expanded (that stays the chevron's job), so only the ancestors are
        // targets. Depth matches the chain index: the root's children are at 1.
        let targets: Vec<(PathBuf, usize)> = tree::ancestor_chain(&current, &root)
            .into_iter()
            .enumerate()
            .skip(1)
            .filter(|(_, path)| path != &current)
            .map(|(depth, path)| (path, depth + 1))
            .collect();

        if targets.is_empty() {
            // The current directory is a direct child of the root; nothing to
            // expand, but it may still need scrolling.
            self.last_revealed = Some(current);
            self.scroll_tree_to_current();
            cx.notify();
            return;
        }

        // The root's children may still be loading; `load_tree_children` calls
        // back into this method once they exist, so record the target only after
        // the walk actually ran.
        if self.tree_nodes.is_empty() {
            return;
        }
        self.last_revealed = Some(current);

        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        self._tree_task = Some(cx.spawn_in(window, async move |this, cx| {
            for (directory, child_depth) in targets {
                let already_loaded = this
                    .update(cx, |this, _| {
                        TreeNode::find_mut(&mut this.tree_nodes, &directory)
                            .map(|node| node.loaded)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true);

                let children = if already_loaded {
                    None
                } else {
                    Some(tree::read_directories(&fs, &directory, child_depth, show_hidden).await)
                };

                let stop = this
                    .update(cx, |this, _| {
                        if this.tree_root.as_deref() != Some(root.as_path()) {
                            return true;
                        }
                        if let Some(node) = TreeNode::find_mut(&mut this.tree_nodes, &directory) {
                            if let Some(children) = children {
                                node.children = children;
                                node.loaded = true;
                            }
                            node.expanded = true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if stop {
                    return;
                }
            }

            this.update(cx, |this, cx| {
                if this.tree_root.as_deref() != Some(root.as_path()) {
                    return;
                }
                this.scroll_tree_to_current();
                cx.notify();
            })
            .ok();
        }));
    }

    /// Scrolls the tree so the current directory's row is visible.
    ///
    /// The tree is a plain column (not a `uniform_list`), so the offset is
    /// computed from the row index and the fixed row pitch.
    fn scroll_tree_to_current(&self) {
        let Some(current) = self.current_dir.as_deref() else {
            return;
        };
        let rows = tree::visible_rows(&self.tree_nodes);
        let Some(index) = rows.iter().position(|node| node.path == current) else {
            return;
        };

        // `py(6)` padding of the list plus one row per entry.
        let offset = metrics::TREE_PANE_PADDING + metrics::TREE_ROW * index as f32;
        let max = f32::from(self.tree_scroll.max_offset().y);
        // Leave a row of context above, and never scroll past the end.
        let y = -(offset - metrics::TREE_ROW).clamp(0., max.max(0.));
        self.tree_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(y)));
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
        self._tree_loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let children = tree::read_directories(&fs, &directory, depth + 1, show_hidden).await;
            this.update_in(cx, |this, window, cx| {
                if this.tree_root.as_deref() == Some(directory.as_path()) {
                    this.tree_nodes = children;
                    // The root's children only exist now, so this is the first
                    // moment at which the current directory can be revealed.
                    this.reveal_in_tree(window, cx);
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
    ///
    /// Only the ancestors are expanded, never the current directory itself:
    /// clicking a tree row only navigates, and expanding a folder is the job of
    /// its chevron (`>`), like GNOME Files' tree sidebar. A folder that was
    /// opened before therefore keeps whatever expansion state the user gave it.
    fn sync_tree_expansion(&mut self) {
        let Some(root) = self.tree_root.clone() else {
            return;
        };
        let Some(current) = self.current_dir.clone() else {
            return;
        };
        let chain = tree::ancestor_chain(&current, &root);
        for path in chain.iter().skip(1) {
            if path == &current {
                continue;
            }
            // Auto-expanding a node whose children were never read would flip
            // its chevron down while showing nothing, so only loaded nodes are
            // expanded; the rest stay to be opened with the chevron.
            if let Some(node) = TreeNode::find_mut(&mut self.tree_nodes, path) {
                if node.loaded {
                    node.expanded = true;
                }
            }
        }
    }

    /// Removes a directory from the tree, e.g. after it was deleted.
    pub(crate) fn prune_tree(&mut self, path: &Path) {
        TreeNode::remove(&mut self.tree_nodes, path);
    }
}
