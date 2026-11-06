//! The sidebar state: the "Places" list and the folder tree.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::tree::{self, TreeNode};
use crate::model::xdg::home_dir;
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
                // Staying inside the root is the common case (browsing around
                // home), and it still has to *reveal* the new folder: the
                // ancestors that were never opened are read lazily and the row
                // is scrolled into view. Returning here without this used to
                // make the tree follow only once, when the root finished
                // loading.
                self.reveal_in_tree(window, cx);
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

    /// Makes the tree mirror the current directory: expands the path to it and
    /// folds every branch that is not on that path.
    ///
    /// This is the "navigate to the folder" behaviour: opening a folder in the
    /// view highlights it in the tree and expanding/collapsing follows the
    /// path, so going back up folds the branch again instead of leaving a
    /// growing set of expanded folders behind. Branches that were expanded by
    /// hand are folded too — the tree is a mirror of the current location, not a
    /// history of what was opened.
    ///
    /// The walk is lazy: every level is read on demand, one directory at a time,
    /// and `last_revealed` keeps it from running on every render.
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
        let chain = tree::ancestor_chain(&current, &root);
        let targets: Vec<(PathBuf, usize)> = chain
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, path)| *path != &current)
            .map(|(depth, path)| (path.clone(), depth + 1))
            .collect();

        // Fold everything that is off the path first; this needs no I/O, so the
        // tree already looks right before the lazy expansion finishes.
        let keep: Vec<PathBuf> = targets.iter().map(|(path, _)| path.clone()).collect();
        tree::collapse_all_except(&mut self.tree_nodes, &keep);

        if targets.is_empty() {
            // The current directory is a direct child of the root; nothing to
            // expand, but it may still need scrolling.
            self.last_revealed = Some(current);
            self.defer_scroll_tree_to_current(window, cx);
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

            this.update_in(cx, |this, window, cx| {
                if this.tree_root.as_deref() != Some(root.as_path()) {
                    return;
                }
                this.defer_scroll_tree_to_current(window, cx);
            })
            .ok();
        }));
    }

    /// Scrolls the tree so the current directory's row is visible.
    ///
    /// `scroll_to_item` scrolls the least amount that makes the row fully
    /// visible (and does nothing when it already is), using the bounds GPUI
    /// recorded for the scroll container's children. That is why the rows are
    /// direct children of the element tracked by `tree_scroll`, and why this
    /// does not compute an offset from a row pitch: it would neither know the
    /// viewport height nor the real padding.
    pub(crate) fn scroll_tree_to_current(&self) {
        let Some(current) = self.current_dir.as_deref() else {
            return;
        };
        let rows = tree::visible_rows(&self.tree_nodes);
        let Some(index) = rows.iter().position(|node| node.path == current) else {
            return;
        };
        self.tree_scroll.scroll_to_item(index);
    }

    /// Scrolls the tree into view on the next frame.
    ///
    /// The rows only exist after the frame that paints the expansion, so the
    /// scroll has to wait for it.
    fn defer_scroll_tree_to_current(&self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = cx.entity().downgrade();
        cx.notify();
        window.defer(cx, move |_window, cx| {
            panel
                .update(cx, |this, cx| {
                    this.scroll_tree_to_current();
                    cx.notify();
                })
                .ok();
        });
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
