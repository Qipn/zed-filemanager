//! The `FileManagerPanel` entity: state, navigation, file operations and the
//! `Panel` trait implementation. Rendering lives in `render.rs`, the menu
//! builders in `panel/menus.rs`.

mod file_ops;
mod menus;
mod search;
mod selection;
mod sidebar_state;
mod transfer;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use fs::Fs;
use gpui::{
    Action, App, AsyncWindowContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, IntoElement, Pixels, Point, Render, SharedString, Subscription, Task, TaskExt,
    WeakEntity, Window, px,
};
use project::Project;
use ui::prelude::*;
use util::ResultExt;
use workspace::{
    OpenOptions, OpenVisible, Workspace,
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

/// The panel's internal drag payload: paths being dragged between entries.
#[derive(Clone, Debug)]
pub(crate) struct DraggedPaths(pub(crate) Vec<PathBuf>);

/// The drag payload of the sidebar resizer.
///
/// Zed's dock does the same thing with `DraggedDock`
/// (`crates/workspace/src/dock.rs`): the handle only starts a drag, GPUI owns
/// the drag lifecycle, and the container resizes on every `on_drag_move`.
/// Releasing the mouse therefore always ends the drag.
#[derive(Clone)]
pub(crate) struct SidebarResizeDrag;

impl Render for SidebarResizeDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Small floating view shown while dragging entries.
pub(crate) struct DragGhost {
    pub(crate) labels: Vec<SharedString>,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .px_2()
            .py_1()
            .rounded_xs()
            .bg(cx.theme().colors().element_selected)
            .child(
                Label::new(self.labels.first().cloned().unwrap_or_default())
                    .single_line()
                    .size(LabelSize::Small),
            )
            .when(self.labels.len() > 1, |label| {
                label.child(
                    Label::new(format!("+{}", self.labels.len() - 1))
                        .single_line()
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
    }
}

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

    fn set_current_dir(
        &mut self,
        directory: PathBuf,
        selected: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.using_fallback_dir = false;
        self.current_dir = Some(directory.clone());
        self.selected.clear();
        self.selection_anchor = None;
        // Navigating leaves the search, like GNOME Files restores the location
        // it had before the search.
        self.search_query = None;
        self.clear_search_results();
        if let Some(path) = selected {
            self.selected.push(path);
        }
        self.history.truncate(self.history_index + 1);
        if self.history.last() != Some(&directory) {
            self.history.push(directory);
        }
        self.history_index = self.history.len().saturating_sub(1);
        self.load_directory(window, cx);
        self.refresh_tree(window, cx);
        cx.notify();
    }

    pub(crate) fn navigate_to(
        &mut self,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_dir.as_ref() == Some(&directory) {
            return;
        }
        self.set_current_dir(directory, None, window, cx);
    }

    /// Reveals a path in this panel: selects the file, or opens the directory.
    pub fn reveal_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.is_dir() {
            self.set_current_dir(path, None, window, cx);
        } else {
            let parent = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone());
            self.set_current_dir(parent, Some(path), window, cx);
        }
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.history_index -= 1;
        self.current_dir = self.history.get(self.history_index).cloned();
        self.selected.clear();
        self.selection_anchor = None;
        self.using_fallback_dir = false;
        self.load_directory(window, cx);
        cx.notify();
    }

    pub(crate) fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        self.history_index += 1;
        self.current_dir = self.history.get(self.history_index).cloned();
        self.selected.clear();
        self.selection_anchor = None;
        self.using_fallback_dir = false;
        self.load_directory(window, cx);
        cx.notify();
    }

    pub(crate) fn go_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self
            .current_dir
            .as_ref()
            .and_then(|directory| directory.parent())
            .map(Path::to_path_buf)
        else {
            return;
        };
        self.set_current_dir(parent, None, window, cx);
    }

    pub(crate) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_directory(window, cx);
    }

    /// Toggles hidden-entry visibility and reloads.
    pub(crate) fn toggle_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.load_directory(window, cx);
        cx.notify();
    }

    /// Sorts by name, toggling the direction when the column is already active.
    pub(crate) fn sort_by_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_name() {
            self.sort_mode.toggled()
        } else {
            SortMode::NameAscending
        };
        self.load_directory(window, cx);
        cx.notify();
    }

    /// Sorts by modification date.
    pub(crate) fn sort_by_modified(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_modified() {
            self.sort_mode.toggled()
        } else {
            SortMode::ModifiedAscending
        };
        self.load_directory(window, cx);
        cx.notify();
    }

    /// Sorts by size.
    pub(crate) fn sort_by_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sort_mode = if self.sort_mode.is_size() {
            self.sort_mode.toggled()
        } else {
            SortMode::SizeAscending
        };
        self.load_directory(window, cx);
        cx.notify();
    }

    /// Enters path-bar editing with the current directory prefilled.
    ///
    /// The location slot is shared with the search (like GNOME Files), so
    /// opening the editable path leaves the search mode.
    pub(crate) fn begin_path_edit(&mut self, cx: &mut Context<Self>) {
        self.search_query = None;
        self.path_edit = Some(self.current_dir_label().to_string());
        cx.notify();
    }

    pub(crate) fn cancel_path_edit(&mut self, cx: &mut Context<Self>) {
        self.path_edit = None;
        cx.notify();
    }

    pub(crate) fn path_edit_input(&mut self, key: &str, modifiers: gpui::Modifiers) {
        let Some(buffer) = self.path_edit.as_mut() else {
            return;
        };
        let _ = modifiers;
        match key {
            "space" => buffer.push(' '),
            key if key.len() == 1 => buffer.push_str(key),
            _ => {}
        }
    }

    pub(crate) fn path_edit_backspace(&mut self) {
        if let Some(buffer) = self.path_edit.as_mut() {
            buffer.pop();
        }
    }

    /// Resolves the typed path and jumps to it.
    ///
    /// Supports `~`, relative paths (against the current directory) and symlink-
    /// free lexical joins. A directory is navigated into; a file selects it in
    /// its parent; a missing path keeps the editor open with the error shown.
    pub(crate) fn confirm_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(raw) = self.path_edit.take() else {
            return;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            cx.notify();
            return;
        }

        let mut expanded: PathBuf = if let Some(stripped) = trimmed.strip_prefix("~") {
            util::paths::home_dir().join(stripped.trim_start_matches('/'))
        } else {
            trimmed.into()
        };
        if expanded.is_relative() {
            match &self.current_dir {
                Some(directory) => expanded = directory.join(expanded),
                None => {
                    self.error = Some("No folder open; enter an absolute path".into());
                    self.path_edit = Some(raw);
                    cx.notify();
                    return;
                }
            }
        }
        let target = util::paths::normalize_path(&expanded);

        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let is_dir = fs.is_dir(&target).await;
            let is_file = !is_dir && fs.is_file(&target).await;
            this.update_in(cx, |this, window, cx| {
                if is_dir {
                    this.error = None;
                    this.selected.clear();
                    this.selection_anchor = None;
                    this.set_current_dir(target, None, window, cx);
                } else if is_file {
                    let parent = target
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| target.clone());
                    this.error = None;
                    this.selected.clear();
                    this.selected.push(target.clone());
                    this.set_current_dir(parent, None, window, cx);
                } else {
                    this.error =
                        Some(format!("No such directory or file: {}", target.display()).into());
                    this.path_edit = Some(raw);
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Jump target for a breadcrumb segment.
    pub(crate) fn breadcrumb_targets(&self) -> Vec<(PathBuf, SharedString)> {
        let Some(directory) = &self.current_dir else {
            return Vec::new();
        };
        let mut segments = Vec::new();
        let mut accumulator = PathBuf::new();
        for component in directory.components() {
            use std::path::Component;
            match component {
                Component::RootDir => {
                    accumulator.push(std::path::MAIN_SEPARATOR.to_string());
                    segments.push((accumulator.clone(), "/".into()));
                }
                Component::Prefix(prefix) => {
                    accumulator.push(prefix.as_os_str());
                    segments.push((
                        accumulator.clone(),
                        prefix.as_os_str().to_string_lossy().into(),
                    ));
                }
                component => {
                    accumulator.push(component.as_os_str());
                    segments.push((
                        accumulator.clone(),
                        component.as_os_str().to_string_lossy().into(),
                    ));
                }
            }
        }
        segments
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let options = OpenOptions {
            visible: Some(OpenVisible::None),
            ..Default::default()
        };
        let task = workspace.update(cx, |workspace, cx| {
            workspace.open_abs_path(path, options, window, cx)
        });
        task.detach_and_log_err(cx);
    }

    pub(crate) fn open_entry(
        &mut self,
        entry: DirEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.is_dir {
            self.navigate_to(entry.path, window, cx);
        } else {
            self.open_file(entry.path, window, cx);
        }
    }

    /// Builds and deploys a breadcrumb dropdown for `anchor`: the first entry
    /// navigates to `anchor` itself, then every directory sibling (i.e. child
    /// of `anchor`'s parent) is listed for a lateral switch. Called for a
    /// *click* on a breadcrumb segment.
    pub(crate) fn deploy_breadcrumb_menu(
        &mut self,
        anchor: PathBuf,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.weak_entity();
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut dirs = read_entries(fs, anchor.clone(), true, SortMode::default())
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| entry.is_dir)
                .collect::<Vec<_>>();
            dirs.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

            this.update_in(cx, |this, window, cx| {
                if this.current_dir.as_deref() != Some(anchor.as_path()) {
                    // Navigated elsewhere while loading; skip the menu.
                    return;
                }
                let siblings_parent = anchor
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| anchor.clone());
                let focus = this.focus_handle.clone();
                let menu = ui::ContextMenu::build(window, cx, |menu, _, _| {
                    let mut menu = menu
                        .context(focus)
                        .entry("跳转到此目录", None, {
                            let panel = panel.clone();
                            let anchor = anchor.clone();
                            move |window, cx| {
                                panel
                                    .update(cx, |state, cx| {
                                        state.navigate_to(anchor.clone(), window, cx);
                                    })
                                    .ok();
                            }
                        })
                        .separator();
                    for dir in dirs {
                        let target = siblings_parent
                            .join(dir.path.file_name().expect("read_dir yields file names"));
                        menu = menu.entry(dir.name.clone(), None, {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel
                                    .update(cx, |state, cx| {
                                        state.navigate_to(target.clone(), window, cx);
                                    })
                                    .ok();
                            }
                        });
                    }
                    menu
                });
                window.focus(&menu.focus_handle(cx), cx);
                let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
                    this.context_menu.take();
                    cx.notify();
                });
                this.context_menu = Some(DeployedContextMenu {
                    menu,
                    position,
                    _subscription: subscription,
                });
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
    pub(crate) fn current_dir_label(&self) -> SharedString {
        self.current_dir
            .as_ref()
            .map(|directory| directory.display().to_string().into())
            .unwrap_or_else(|| "No folder open".into())
    }

    /// Sets the view mode.
    pub(crate) fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        cx.notify();
    }

    /// Expands or collapses a directory in the folder view.
    ///
    /// The children are read the first time the directory is opened, like
    /// OpenXplorer's expandable folders.
    pub(crate) fn toggle_entry_expanded(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.expanded_dirs.remove(path).is_some() {
            cx.notify();
            return;
        }

        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let sort_mode = self.sort_mode;
        let path = path.to_path_buf();
        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let children = read_entries(fs, path.clone(), show_hidden, sort_mode).await;
            this.update(cx, |this, cx| {
                match children {
                    Ok(children) => {
                        this.expanded_dirs.insert(path.clone(), children);
                        this.error = None;
                    }
                    Err(error) => {
                        this.error = Some(format!("{error}").into());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Whether a directory is expanded in the folder view.
    pub(crate) fn is_entry_expanded(&self, path: &Path) -> bool {
        self.expanded_dirs.contains_key(path)
    }

    /// The icon size of the grid view, in logical pixels.
    pub(crate) fn icon_size(&self) -> f32 {
        metrics::ICON_SIZES[self.icon_size.min(metrics::ICON_SIZES.len() - 1)]
    }

    /// Makes the icons of the grid view one step larger.
    pub(crate) fn increase_icon_size(&mut self, cx: &mut Context<Self>) {
        if self.icon_size + 1 < metrics::ICON_SIZES.len() {
            self.icon_size += 1;
            cx.notify();
        }
    }

    /// Makes the icons of the grid view one step smaller.
    pub(crate) fn decrease_icon_size(&mut self, cx: &mut Context<Self>) {
        if self.icon_size > 0 {
            self.icon_size -= 1;
            cx.notify();
        }
    }

    /// Shows or hides the details pane.
    pub(crate) fn toggle_details_pane(&mut self, cx: &mut Context<Self>) {
        self.details_pane_visible = !self.details_pane_visible;
        cx.notify();
    }

    /// Shows or hides the sidebar.
    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Sets the width of the sidebar, clamped to OpenXplorer's range.
    pub(crate) fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.sidebar_width = width.clamp(140., 560.);
        cx.notify();
    }

    /// Ends a sidebar drag. Kept for callers that want to cancel one; the
    /// drag itself is owned by GPUI (see `SidebarResizeDrag`), so nothing has
    /// to be cleaned up here.
    pub(crate) fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        self.sidebar_width = metrics::SIDEBAR_WIDTH;
        cx.notify();
    }

    // ---- Responsive layout ----

    /// Records the width the panel was laid out at.
    ///
    /// OpenXplorer adapts its layout to the window width at 1190, 960 and
    /// 680 px (`native/docs/ui-spec.md` §4). Inside a Zed dock the panel can be
    /// much narrower than the window, so the same thresholds are applied to the
    /// panel's own width instead.
    pub(crate) fn set_content_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = width.round();
        if self.content_width == Some(width) {
            return;
        }
        self.content_width = Some(width);
        cx.notify();
    }

    /// The width used for the breakpoints, defaulting to a wide panel.
    fn breakpoint_width(&self) -> f32 {
        self.content_width.unwrap_or(1200.)
    }

    /// Whether the sidebar has room right now. Below 520 px the folder view
    /// needs the space more than the navigation pane does.
    pub(crate) fn is_sidebar_shown(&self) -> bool {
        self.sidebar_visible && self.breakpoint_width() >= 520.
    }

    /// Whether the details pane fits. OpenXplorer hides it at ≤ 960.
    pub(crate) fn is_details_pane_shown(&self) -> bool {
        self.details_pane_visible && self.breakpoint_width() >= 900.
    }

    /// The effective sidebar width: the user's width, but never more than a
    /// third of a narrow panel.
    pub(crate) fn effective_sidebar_width(&self) -> f32 {
        let width = self.breakpoint_width();
        if width < 700. {
            self.sidebar_width.min((width * 0.42).max(140.))
        } else {
            self.sidebar_width
        }
    }

    /// Whether the `Date modified` column fits.
    pub(crate) fn is_modified_column_shown(&self) -> bool {
        self.breakpoint_width() >= 640.
    }

    /// Whether the `Type` column fits (OpenXplorer drops it at ≤ 680).
    pub(crate) fn is_type_column_shown(&self) -> bool {
        self.breakpoint_width() >= 760.
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
