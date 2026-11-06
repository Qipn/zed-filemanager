//! The `FileManagerPanel` entity: state, navigation, file operations and the
//! `Panel` trait implementation. Rendering lives in `render.rs`, the menu
//! builders in `panel/menus.rs`.

mod file_ops;
mod layout;
mod menus;
mod navigation;
mod search;
mod selection;
mod sidebar_state;
mod transfer;
mod view_state;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use fs::Fs;
use gpui::{
    Action, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Pixels, Point, Render, SharedString, Subscription, Task, WeakEntity, Window, px,
};
use project::Project;
use ui::prelude::*;
use util::ResultExt;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::ToggleFocus;
use crate::model::clipboard::{Clipboard, RenameState};
use crate::model::entries::{DirEntry, SortMode, read_entries};
use crate::model::tree::TreeNode;
use crate::model::undo::UndoManager;
use crate::ui::ox_theme::metrics;
use crate::ui::places::PlaceEntry;
use crate::ui::render as render_panel;
use crate::ui::views::ViewMode;

/// A transfer waiting for the user to decide how name conflicts are resolved.
pub(crate) struct PendingTransfer {
    /// The paths being transferred.
    pub(crate) sources: Vec<PathBuf>,
    /// The planned destinations, parallel to `sources`.
    pub(crate) targets: Vec<PathBuf>,
    /// `true` copies the sources, `false` moves them.
    pub(crate) copy: bool,
    /// Indices into `sources` whose target already exists.
    pub(crate) conflicts: Vec<usize>,
    /// `true` when the transfer came from the clipboard's *cut* list, so the
    /// clipboard can be cleared afterwards.
    pub(crate) from_cut: bool,
}

/// A context menu deployed at the mouse position.
pub(crate) struct DeployedContextMenu {
    pub(crate) menu: Entity<ui::ContextMenu>,
    pub(crate) position: Point<Pixels>,
    _subscription: Subscription,
}

/// A dock panel that browses directories and opens files in Zed.
pub struct FileManagerPanel {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) project: Entity<Project>,
    pub(crate) workspace: WeakEntity<Workspace>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) position: DockPosition,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) using_fallback_dir: bool,
    pub(crate) entries: Vec<DirEntry>,
    /// Selected paths, in selection order. The last entry is the "active" one
    /// that keyboard navigation and rename operate on.
    pub(crate) selected: Vec<PathBuf>,
    /// Index of the row the current selection started from (shift-click range).
    pub(crate) selection_anchor: Option<usize>,
    pub(crate) history: Vec<PathBuf>,
    pub(crate) history_index: usize,
    pub(crate) loading_generation: u64,
    pub(crate) _loading_task: Option<Task<()>>,
    pub(crate) _operation_task: Option<Task<()>>,
    pub(crate) error: Option<SharedString>,
    pub(crate) clipboard: Option<Clipboard>,
    pub(crate) rename_state: Option<RenameState>,
    pub(crate) context_menu: Option<DeployedContextMenu>,
    pub(crate) undo_manager: UndoManager,
    pub(crate) show_hidden: bool,
    pub(crate) sort_mode: SortMode,
    /// Active path-bar edit: the typed buffer. `Some` means editing.
    pub(crate) path_edit: Option<String>,
    /// The entry currently being hovered by a drag, for drop highlight.
    pub(crate) drag_target: Option<PathBuf>,
    /// A transfer waiting for conflict resolution.
    pub(crate) pending_transfer: Option<PendingTransfer>,
    /// Which view shows the contents of the current directory.
    pub(crate) view_mode: ViewMode,
    /// Whether the sidebar (places + folder tree) is shown.
    pub(crate) sidebar_visible: bool,
    /// Whether the details pane on the right is shown.
    pub(crate) details_pane_visible: bool,
    /// The search query of the search box; `None` when the box is closed.
    pub(crate) search_query: Option<String>,
    /// The hits of the last completed search, if any.
    pub(crate) search_results: Option<crate::model::search::SearchResults>,
    /// Whether a search is currently running.
    pub(crate) search_running: bool,
    /// Discards the results of superseded searches.
    pub(crate) search_generation: u64,
    /// The children of the directories that are expanded in the folder view.
    pub(crate) expanded_dirs: std::collections::HashMap<PathBuf, Vec<DirEntry>>,
    /// The index of the icon size in the grid view.
    pub(crate) icon_size: usize,
    /// Width of the sidebar in pixels.
    pub(crate) sidebar_width: f32,
    /// The places list of the sidebar.
    pub(crate) places: Vec<PlaceEntry>,
    /// The root of the folder tree.
    pub(crate) tree_root: Option<PathBuf>,
    /// The direct children of the tree root.
    pub(crate) tree_nodes: Vec<TreeNode>,
    /// The width the panel was last laid out at, used for the responsive
    /// breakpoints. `None` until the first frame.
    pub(crate) content_width: Option<f32>,
}

// The drag payloads live in their own module; re-exported so the views keep
// referring to them through the panel.
pub(crate) use crate::ui::drag::{DraggedPaths, SidebarResizeDrag};

impl FileManagerPanel {
    /// Creates the panel for a workspace.
    pub fn new(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let project = workspace.project().clone();
        let fs = project.read(cx).fs().clone();
        let workspace_handle = cx.weak_entity();
        let (current_dir, using_fallback_dir) = match Self::initial_directory(project.read(cx), cx)
        {
            Some(directory) => (Some(directory), false),
            None => (std::env::current_dir().ok(), true),
        };

        let panel = cx.new(|cx| Self {
            fs,
            project: project.clone(),
            workspace: workspace_handle,
            focus_handle: cx.focus_handle(),
            position: DockPosition::Left,
            current_dir,
            using_fallback_dir,
            entries: Vec::new(),
            selected: Vec::new(),
            selection_anchor: None,
            history: Vec::new(),
            history_index: 0,
            loading_generation: 0,
            _loading_task: None,
            _operation_task: None,
            error: None,
            clipboard: None,
            rename_state: None,
            context_menu: None,
            undo_manager: UndoManager::new(),
            show_hidden: false,
            sort_mode: SortMode::default(),
            path_edit: None,
            drag_target: None,
            pending_transfer: None,
            view_mode: ViewMode::Details,
            sidebar_visible: true,
            details_pane_visible: false,
            search_query: None,
            search_results: None,
            search_running: false,
            search_generation: 0,
            expanded_dirs: std::collections::HashMap::new(),
            icon_size: metrics::DEFAULT_ICON_SIZE,
            sidebar_width: metrics::SIDEBAR_WIDTH,
            places: Vec::new(),
            tree_root: None,
            tree_nodes: Vec::new(),
            content_width: None,
        });

        panel.update(cx, |panel, cx| {
            cx.subscribe_in(&project, window, |this, project, event, window, cx| {
                let should_adopt = match event {
                    project::Event::WorktreeAdded(_)
                    | project::Event::WorktreeRemoved(_)
                    | project::Event::WorktreeOrderChanged
                    | project::Event::WorktreePathsChanged { .. } => true,
                    _ => false,
                };
                if should_adopt {
                    this.adopt_initial_directory(project.clone(), window, cx);
                }
            })
            .detach();

            if let Some(dir) = panel.current_dir.clone() {
                panel.history.push(dir);
                panel.load_directory(window, cx);
            }
            // The sidebar needs the places list and the folder tree.
            panel.refresh_places(cx);
            panel.refresh_tree(window, cx);
        });

        panel
    }

    /// Loads the panel for the workspace during startup.
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            Ok(FileManagerPanel::new(workspace, window, cx))
        })?
    }

    fn initial_directory(project: &Project, cx: &App) -> Option<PathBuf> {
        project.visible_worktrees(cx).find_map(|worktree| {
            let worktree = worktree.read(cx);
            if worktree.root_entry().is_some_and(|entry| entry.is_dir()) {
                Some(worktree.abs_path().to_path_buf())
            } else {
                None
            }
        })
    }

    fn adopt_initial_directory(
        &mut self,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_dir.is_some() && !self.using_fallback_dir {
            return;
        }

        let Some(directory) = Self::initial_directory(project.read(cx), cx) else {
            return;
        };

        self.current_dir = Some(directory.clone());
        self.selected.clear();
        self.selection_anchor = None;
        self.history.clear();
        self.history.push(directory);
        self.history_index = 0;
        self.using_fallback_dir = false;
        self.load_directory(window, cx);
        cx.notify();
    }

    fn load_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };

        self.loading_generation = self.loading_generation.wrapping_add(1);
        let generation = self.loading_generation;
        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let sort_mode = self.sort_mode;

        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read_entries(fs, directory, show_hidden, sort_mode).await;
            this.update(cx, |this, cx| {
                if this.loading_generation != generation {
                    return;
                }
                match result {
                    Ok(entries) => {
                        this.entries = entries;
                        this.error = None;
                    }
                    Err(error) => {
                        this.entries.clear();
                        this.error = Some(format!("{error}").into());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    pub(crate) fn copy_selected(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if !paths.is_empty() {
            self.clipboard = Some(Clipboard::Copied(paths));
            cx.notify();
        }
    }

    pub(crate) fn cut_selected(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        if !paths.is_empty() {
            self.clipboard = Some(Clipboard::Cut(paths));
            cx.notify();
        }
    }

    pub(crate) fn paste_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clipboard) = self.clipboard.clone() else {
            return;
        };
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let sources: Vec<PathBuf> = clipboard
            .paths()
            .iter()
            .filter(|source| {
                source
                    .file_name()
                    .is_some_and(|name| directory.join(name) != **source)
            })
            .cloned()
            .collect();
        if sources.is_empty() {
            return;
        }
        // A cut clipboard moves the entries, a copied one duplicates them.
        let copy = matches!(clipboard, Clipboard::Copied(_));
        let from_cut = matches!(clipboard, Clipboard::Cut(_));
        self.start_transfer(sources, directory, copy, from_cut, window, cx);
    }
}

/// Appends the children of an expanded directory to the visible rows.
fn push_child_rows(
    children: &[DirEntry],
    depth: usize,
    matches: &impl Fn(&str) -> bool,
    expanded: &std::collections::HashMap<PathBuf, Vec<DirEntry>>,
    rows: &mut Vec<(DirEntry, usize)>,
) {
    for child in children {
        if !matches(&child.name) {
            continue;
        }
        rows.push((child.clone(), depth));
        if child.is_dir {
            if let Some(grandchildren) = expanded.get(&child.path) {
                push_child_rows(grandchildren, depth + 1, matches, expanded, rows);
            }
        }
    }
}

impl EventEmitter<PanelEvent> for FileManagerPanel {}

impl Focusable for FileManagerPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for FileManagerPanel {
    fn persistent_name() -> &'static str {
        "File Manager"
    }

    fn panel_key() -> &'static str {
        "FileManagerPanel"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        if self.position_is_valid(position) {
            self.position = position;
            cx.notify();
        }
    }

    fn default_size(&self, _: &Window, _: &App) -> gpui::Pixels {
        px(260.)
    }

    fn min_size(&self, _: &Window, _: &App) -> Option<gpui::Pixels> {
        Some(px(180.))
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Folder)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("File Manager")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            // Do not read the workspace here: dock activation happens while the
            // workspace entity is already being updated, which would panic.
            let project = self.project.clone();
            self.adopt_initial_directory(project, window, cx);
        }
    }

    fn activation_priority(&self) -> u32 {
        2
    }

    fn starts_open(&self, _: &Window, _: &App) -> bool {
        false
    }
}

impl Render for FileManagerPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_panel::render(self, window, cx)
    }
}
