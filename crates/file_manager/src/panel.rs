//! The `FileManagerPanel` entity: state, navigation, file operations and the
//! `Panel` trait implementation. Rendering lives in `render.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use fs::{CreateOptions, Fs, RemoveOptions, RenameOptions};
use gpui::{
    Action, Anchor, App, AsyncWindowContext, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, IntoElement, KeyDownEvent, Pixels, Point, PromptLevel, Render,
    SharedString, Subscription, Task, TaskExt, WeakEntity, Window, anchored, deferred, px,
};
use project::Project;
use ui::prelude::*;
use util::ResultExt;
use workspace::{
    OpenOptions, OpenVisible, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::clipboard::{Clipboard, RenameState};
use crate::conflict::{ConflictPolicy, apply_transfer, conflicting_indices, plan_targets};
use crate::entries::{DirEntry, SortMode, read_entries};
use crate::ox_theme::metrics;
use crate::places::{PlaceEntry, build_places, home_dir};
use crate::tree::{self, TreeNode};
use crate::undo::{Change, HistoryStep, UndoManager};
use crate::views::ViewMode;
use crate::{
    Copy, Cut, Delete, NewFile, NewFolder, Open, Paste, Redo, Refresh, Rename, ToggleFocus, Trash,
    Undo, render as render_panel,
};

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
    pub(crate) fn begin_path_edit(&mut self, cx: &mut Context<Self>) {
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

    /// Dropping internally dragged paths onto a directory `target` (an entry
    /// or the panel background). Ctrl-drag copies; otherwise moves.
    pub(crate) fn drop_dragged(
        &mut self,
        dragged: DraggedPaths,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_target = None;
        // Dropping a selection onto itself (or any selected path) falls back
        // to the current directory.
        let destination = if self.selected.contains(&target) {
            self.current_dir.clone()
        } else {
            Some(target)
        };
        let Some(destination) = destination else {
            return;
        };
        let sources: Vec<PathBuf> = dragged
            .0
            .into_iter()
            .filter(|source| *source != destination)
            .collect();
        if sources.is_empty() {
            return;
        }
        // Holding Ctrl copies the dragged entries instead of moving them.
        let copy = window.modifiers().control;
        self.start_transfer(sources, destination, copy, false, window, cx);
    }

    /// External drop: copies the dragged files into `target`.
    pub(crate) fn drop_external(
        &mut self,
        paths: Vec<PathBuf>,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_target = None;
        // External files are always copied, never moved.
        self.start_transfer(paths, target, true, false, window, cx);
    }

    /// Starts a transfer of `sources` into `destination`.
    ///
    /// Name conflicts are detected first; when any exist the transfer is
    /// suspended and the user is asked how to resolve them (see
    /// [`Self::resolve_conflicts`]).
    pub(crate) fn start_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        destination: PathBuf,
        copy: bool,
        from_cut: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            return;
        }
        let targets = plan_targets(&sources, &destination);
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let conflicts = conflicting_indices(&fs, &targets);
            this.update_in(cx, |this, window, cx| {
                if conflicts.is_empty() {
                    let policies = vec![ConflictPolicy::KeepBoth; targets.len()];
                    this.execute_transfer(sources, targets, copy, from_cut, policies, window, cx);
                } else {
                    this.pending_transfer = Some(PendingTransfer {
                        sources,
                        targets,
                        copy,
                        conflicts,
                        from_cut,
                    });
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    /// Applies the user's conflict resolution to the suspended transfer.
    pub(crate) fn resolve_conflicts(
        &mut self,
        policy: ConflictPolicy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_transfer.take() else {
            return;
        };
        let policies: Vec<ConflictPolicy> = pending
            .targets
            .iter()
            .enumerate()
            .map(|(index, _)| {
                if pending.conflicts.contains(&index) {
                    policy
                } else {
                    ConflictPolicy::KeepBoth
                }
            })
            .collect();
        self.execute_transfer(
            pending.sources,
            pending.targets,
            pending.copy,
            pending.from_cut,
            policies,
            window,
            cx,
        );
    }

    /// Aborts the suspended transfer, leaving the filesystem untouched.
    pub(crate) fn cancel_conflicts(&mut self, cx: &mut Context<Self>) {
        self.pending_transfer = None;
        cx.notify();
    }

    /// Performs the transfer itself, applying `policies` per item.
    fn execute_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
        copy: bool,
        from_cut: bool,
        policies: Vec<ConflictPolicy>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut changes: Vec<Change> = Vec::new();
            let mut written: Vec<PathBuf> = Vec::new();
            let mut error = None;

            for (index, (source, target)) in sources.iter().zip(targets.iter()).enumerate() {
                let policy = policies
                    .get(index)
                    .copied()
                    .unwrap_or(ConflictPolicy::KeepBoth);
                match apply_transfer(&fs, source, target, copy, policy).await {
                    Ok(Some((source, final_target))) => {
                        if copy {
                            changes.push(Change::Created(final_target.clone()));
                        } else {
                            changes.push(Change::Renamed(source, final_target.clone()));
                        }
                        written.push(final_target);
                    }
                    // The item was skipped because of a conflict.
                    Ok(None) => {}
                    Err(err) => {
                        error = Some(format!("{err}"));
                        break;
                    }
                }
            }

            this.update_in(cx, |this, window, cx| {
                this.error = error.map(Into::into);
                if this.error.is_none() {
                    this.undo_manager.record(changes);
                    if from_cut {
                        this.clipboard = None;
                    }
                    this.selected.clear();
                    this.selected.extend(written);
                    this.load_directory(window, cx);
                } else {
                    cx.notify();
                }
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

    /// The rows shown in the folder view: the entries of the current
    /// directory, with every expanded directory followed by its children.
    ///
    /// The second element of the pair is the nesting depth, used for the
    /// indentation of the tree.
    pub(crate) fn visible_rows(&self) -> Vec<(DirEntry, usize)> {
        let query = self
            .search_query
            .as_ref()
            .map(|query| query.to_lowercase())
            .filter(|query| !query.is_empty());
        let matches = |name: &str| match &query {
            Some(query) => name.to_lowercase().contains(query),
            None => true,
        };

        let mut rows = Vec::new();
        for entry in &self.entries {
            if !matches(&entry.name) {
                continue;
            }
            rows.push((entry.clone(), 0));
            if entry.is_dir {
                if let Some(children) = self.expanded_dirs.get(&entry.path) {
                    push_child_rows(children, 1, &matches, &self.expanded_dirs, &mut rows);
                }
            }
        }
        rows
    }

    /// The "active" selected entry (the one rename operates on).
    pub(crate) fn selected_entry(&self) -> Option<DirEntry> {
        let selected = self.selected.last()?;
        self.visible_rows()
            .into_iter()
            .find(|(entry, _)| &entry.path == selected)
            .map(|(entry, _)| entry)
    }

    /// Every selected entry, in directory order.
    pub(crate) fn selected_entries(&self) -> Vec<DirEntry> {
        self.visible_rows()
            .into_iter()
            .filter(|(entry, _)| self.selected.contains(&entry.path))
            .map(|(entry, _)| entry)
            .collect()
    }

    pub(crate) fn is_selected(&self, path: &Path) -> bool {
        self.selected.iter().any(|selected| selected == path)
    }

    pub(crate) fn select_only(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        if let Some((entry, _)) = rows.get(index) {
            self.selected.clear();
            self.selected.push(entry.path.clone());
            self.selection_anchor = Some(index);
            cx.notify();
        }
    }

    pub(crate) fn toggle_select(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        let Some((entry, _)) = rows.get(index) else {
            return;
        };
        if let Some(position) = self
            .selected
            .iter()
            .position(|selected| selected == &entry.path)
        {
            self.selected.remove(position);
        } else {
            self.selected.push(entry.path.clone());
            self.selection_anchor = Some(index);
        }
        cx.notify();
    }

    pub(crate) fn select_range(&mut self, index: usize, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        let anchor = self.selection_anchor.or_else(|| {
            self.selected
                .last()
                .and_then(|path| rows.iter().position(|(entry, _)| &entry.path == path))
        });
        let Some(anchor) = anchor else {
            self.select_only(index, cx);
            return;
        };
        self.selected.clear();
        for index in anchor.min(index)..=anchor.max(index) {
            if let Some((entry, _)) = rows.get(index) {
                self.selected.push(entry.path.clone());
            }
        }
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.last()?;
        self.visible_rows()
            .iter()
            .position(|(entry, _)| &entry.path == selected)
    }

    fn select_relative(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let row_count = self.visible_rows().len();
        if row_count == 0 {
            return;
        }
        let Some(current) = self.selected_index() else {
            let first = if delta < 0 { row_count - 1 } else { 0 };
            self.select_only(first, cx);
            return;
        };
        let next = if delta < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(row_count - 1)
        };
        if extend {
            self.select_range(next, cx);
        } else {
            self.select_only(next, cx);
        }
    }

    pub(crate) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_entry() {
            self.open_entry(entry, window, cx);
        }
    }

    pub(crate) fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        // While the search box is open, all keystrokes feed the query.
        if self.search_query.is_some() {
            match event.keystroke.key.as_str() {
                "escape" => {
                    self.search_query = None;
                }
                "enter" => {}
                "backspace" => {
                    self.search_backspace(cx);
                }
                key => {
                    if !modifiers.control && !modifiers.alt && !modifiers.platform {
                        self.search_input(key, cx);
                    }
                }
            }
            cx.stop_propagation();
            return;
        }
        // While path-bar editing is active, all keystrokes feed the buffer.
        if self.path_edit.is_some() {
            match event.keystroke.key.as_str() {
                "enter" => {
                    self.confirm_path_edit(window, cx);
                }
                "escape" => {
                    self.cancel_path_edit(cx);
                }
                "backspace" => {
                    self.path_edit_backspace();
                }
                key => {
                    if !modifiers.control && !modifiers.alt && !modifiers.platform {
                        self.path_edit_input(key, modifiers);
                    }
                }
            }
            cx.stop_propagation();
            return;
        }
        // While an inline rename is active, all keystrokes feed the buffer.
        if self.rename_state.is_some() {
            match event.keystroke.key.as_str() {
                "enter" => {
                    self.confirm_rename(window, cx);
                }
                "escape" => {
                    self.cancel_rename(cx);
                }
                "backspace" => {
                    if let Some(state) = self.rename_state.as_mut() {
                        state.buffer.pop();
                    }
                }
                key => {
                    if !modifiers.control && !modifiers.alt && !modifiers.platform {
                        let character = if key == "space" {
                            Some(' ')
                        } else if key.len() == 1 {
                            let mut chars = key.chars();
                            let character = chars.next();
                            if modifiers.shift {
                                character.map(|c| c.to_ascii_uppercase())
                            } else {
                                character
                            }
                        } else {
                            None
                        };
                        if let Some(character) = character {
                            if let Some(state) = self.rename_state.as_mut() {
                                state.buffer.push(character);
                            }
                        }
                    }
                }
            }
            cx.stop_propagation();
            return;
        }

        match event.keystroke.key.as_str() {
            "up" => {
                self.select_relative(-1, modifiers.shift, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.select_relative(1, modifiers.shift, cx);
                cx.stop_propagation();
            }
            "home" => {
                self.select_only(0, cx);
                cx.stop_propagation();
            }
            "end" => {
                if !self.entries.is_empty() {
                    self.select_only(self.entries.len() - 1, cx);
                }
                cx.stop_propagation();
            }
            "backspace" => {
                self.go_up(window, cx);
                cx.stop_propagation();
            }
            "f5" => {
                self.refresh(window, cx);
                cx.stop_propagation();
            }
            "delete" => {
                if modifiers.shift {
                    self.confirm_remove(false, window, cx);
                } else {
                    self.confirm_remove(true, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn unique_child_name(&self, base: &str) -> String {
        let mut index = 2;
        let mut candidate = base.to_string();
        while self
            .entries
            .iter()
            .any(|entry| entry.name.to_string() == candidate)
        {
            candidate = format!("{base} {index}");
            index += 1;
        }
        candidate
    }

    pub(crate) fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let path = directory.join(self.unique_child_name("New Folder"));
        let fs = self.fs.clone();
        self.spawn_operation(window, cx, path.clone(), async move {
            fs.create_dir(&path).await
        });
    }

    pub(crate) fn create_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_file_named("New File", window, cx);
    }

    /// Creates an empty file with the given base name.
    ///
    /// The New menu creates a file per kind (`New Markdown document.md`,
    /// `New CSV file.csv`, …) as OpenXplorer does (OPS-001, OPS-002).
    pub(crate) fn create_file_named(
        &mut self,
        base_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };
        let path = directory.join(self.unique_child_name(base_name));
        let fs = self.fs.clone();
        self.spawn_operation(window, cx, path.clone(), async move {
            fs.create_file(&path, CreateOptions::default()).await
        });
    }

    /// Runs a create-type operation on `path`, records it on the undo history
    /// and refreshes the directory.
    fn spawn_operation<F>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        path: PathBuf,
        operation: F,
    ) where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = operation.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.error = None;
                    this.undo_manager
                        .record(vec![Change::Created(path.clone())]);
                    this.selected.clear();
                    this.selected.push(path.clone());
                    this.load_directory(window, cx);
                }
                Err(error) => {
                    this.error = Some(format!("{error}").into());
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    pub(crate) fn confirm_remove(
        &mut self,
        trash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = self.selected_entries();
        if targets.is_empty() {
            return;
        };
        let count = targets.len();
        let summary = match count {
            1 => format!("“{}”", targets[0].name),
            _ => format!("{count} items"),
        };
        let message = if trash {
            format!("Move {summary} to the Trash?")
        } else {
            format!("Delete {summary} permanently?")
        };
        let confirm = if trash { "Move to Trash" } else { "Delete" };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &[confirm, "Cancel"],
            cx,
        );

        let fs = self.fs.clone();
        let paths: Vec<PathBuf> = targets.into_iter().map(|entry| entry.path).collect();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let results = futures::future::join_all(paths.iter().map(|path| async {
                if trash {
                    fs.trash(path, RemoveOptions::default())
                        .await
                        .map(|trash_id| Some(Change::Trashed(path.clone(), trash_id)))
                } else if fs.is_dir(path).await {
                    fs.remove_dir(
                        path,
                        RemoveOptions {
                            recursive: true,
                            ignore_if_not_exists: false,
                        },
                    )
                    .await
                    .map(|_| None)
                } else {
                    fs.remove_file(
                        path,
                        RemoveOptions {
                            recursive: false,
                            ignore_if_not_exists: false,
                        },
                    )
                    .await
                    .map(|_| None)
                }
            }))
            .await;
            this.update_in(cx, |this, window, cx| {
                let mut changes = Vec::new();
                this.error = None;
                for result in results {
                    match result {
                        Ok(Some(change)) => changes.push(change),
                        Ok(None) => {}
                        Err(err) => {
                            this.error = Some(format!("{err}").into());
                            break;
                        }
                    }
                }
                if this.error.is_none() {
                    this.undo_manager.record(changes);
                    this.selected.clear();
                    this.selection_anchor = None;
                    // The deleted directories must disappear from the tree.
                    for path in &paths {
                        this.prune_tree(path);
                    }
                }
                this.load_directory(window, cx);
            })
            .log_err();
        }));
    }

    pub(crate) fn undo_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_history_step(window, cx, HistoryStep::Undo);
    }

    pub(crate) fn redo_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_history_step(window, cx, HistoryStep::Redo);
    }

    /// Executes the inverse of the newest change on the given stack, then
    /// records the resulting state on the opposite stack.
    fn run_history_step(&mut self, window: &mut Window, cx: &mut Context<Self>, step: HistoryStep) {
        let can_run = match step {
            HistoryStep::Undo => self.undo_manager.can_undo(),
            HistoryStep::Redo => self.undo_manager.can_redo(),
        };
        if !can_run {
            return;
        }
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let popped = this
                .update(cx, |this, _cx| match step {
                    HistoryStep::Undo => this.undo_manager.pop_undo(),
                    HistoryStep::Redo => this.undo_manager.pop_redo(),
                })
                .ok()
                .flatten();
            let Some(change) = popped else {
                return;
            };

            let result = crate::undo::execute_operation(&*fs, change.to_inverse()).await;

            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(new_change) => {
                        this.undo_manager.commit_inverse(step, new_change);
                        this.error = None;
                    }
                    Err(error) => {
                        this.undo_manager.restore_step(step, change);
                        this.error = Some(format!("{error}").into());
                    }
                }
                this.load_directory(window, cx);
            })
            .log_err();
        }));
    }
    pub(crate) fn rename_selected(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        self.rename_state = Some(RenameState {
            source: entry.path.clone(),
            buffer: entry.name.to_string(),
        });
        cx.notify();
    }

    fn confirm_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.rename_state.take() else {
            return;
        };
        let new_name = state.buffer.trim().to_string();
        if new_name.is_empty() {
            cx.notify();
            return;
        }
        let Some(directory) = state.source.parent().map(|parent| parent.to_path_buf()) else {
            return;
        };
        let target = directory.join(&new_name);
        if target == state.source {
            cx.notify();
            return;
        }
        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = fs
                .rename(
                    &state.source,
                    &target,
                    RenameOptions {
                        overwrite: false,
                        ignore_if_exists: true,
                        create_parents: false,
                    },
                )
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.error = None;
                    this.selected.clear();
                    this.selected.push(target.clone());
                    this.load_directory(window, cx);
                }
                Err(error) => {
                    this.error = Some(format!("{error}").into());
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    pub(crate) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.rename_state = None;
        cx.notify();
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

    /// Deploys the context menu at the given window position (right-click).
    pub(crate) fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = !self.selected.is_empty();
        let has_clipboard = self.clipboard.is_some();
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        let menu = ui::ContextMenu::build(window, cx, |menu, _, _| {
            menu.context(self.focus_handle.clone())
                .action_disabled_when(!has_selection, "Open", Box::new(Open))
                .separator()
                .action("New Folder", Box::new(NewFolder))
                .action("New File", Box::new(NewFile))
                .action("Refresh", Box::new(Refresh))
                .separator()
                .action_disabled_when(!has_selection, "Rename", Box::new(Rename))
                .action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                .action_disabled_when(!has_clipboard, "Paste", Box::new(Paste))
                .separator()
                .action_disabled_when(!can_undo, "Undo", Box::new(Undo))
                .action_disabled_when(!can_redo, "Redo", Box::new(Redo))
                .separator()
                .action_disabled_when(!has_selection, "Move to Trash", Box::new(Trash))
                .action_disabled_when(!has_selection, "Delete Permanently", Box::new(Delete))
        });
        window.focus(&menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu = Some(DeployedContextMenu {
            menu,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Renders the deployed context menu, if any.
    pub(crate) fn render_context_menu(&self) -> Option<impl IntoElement> {
        let deployed = self.context_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(deployed.position)
                    .anchor(Anchor::TopLeft)
                    .child(deployed.menu.clone()),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Deploys a menu built by `build` at the given position.
    fn deploy_menu(
        &mut self,
        position: Point<Pixels>,
        build: impl FnOnce(
            ui::ContextMenu,
            &mut Window,
            &mut Context<ui::ContextMenu>,
        ) -> ui::ContextMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = ui::ContextMenu::build(window, cx, build);
        window.focus(&menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu = Some(DeployedContextMenu {
            menu,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// The "New" menu of the command bar.
    ///
    /// Mirrors OpenXplorer's `openNewMenu`
    /// (`native/crates/ox-app/src/window/command_bar/menus.rs`): a folder, a
    /// plain text document, a free file, then the typed documents.
    pub(crate) fn deploy_new_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();

        // Each typed entry creates an empty file with the matching name.
        let typed = [
            ("Markdown document", "New Markdown document.md"),
            ("CSV file", "New CSV file.csv"),
            ("JSON file", "New JSON file.json"),
            ("HTML document", "New HTML document.html"),
        ];

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu
                    .entry("Folder", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.create_folder(window, cx));
                        }
                    })
                    .entry("Text document", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.create_file_named("New Text Document.txt", window, cx)
                            });
                        }
                    })
                    .entry("File", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.create_file(window, cx));
                        }
                    })
                    .separator();

                for (label, base_name) in typed {
                    let panel = panel.clone();
                    menu = menu.entry(label, None, move |window, cx| {
                        panel.update(cx, |state, cx| {
                            state.create_file_named(base_name, window, cx)
                        });
                    });
                }

                menu
            },
            window,
            cx,
        );
    }

    /// The "Sort" menu of the command bar.
    pub(crate) fn deploy_sort_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let sort_mode = self.sort_mode;
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.entry(
                    if sort_mode.is_name() {
                        "Name ✓"
                    } else {
                        "Name"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.sort_by_name(window, cx));
                        }
                    },
                )
                .entry(
                    if sort_mode.is_modified() {
                        "Date modified ✓"
                    } else {
                        "Date modified"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.sort_by_modified(window, cx));
                        }
                    },
                )
                .entry(
                    if sort_mode.is_size() {
                        "Size ✓"
                    } else {
                        "Size"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.sort_by_size(window, cx));
                        }
                    },
                )
            },
            window,
            cx,
        );
    }

    /// The "View" menu of the command bar.
    pub(crate) fn deploy_view_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let view_mode = self.view_mode;
        let show_hidden = self.show_hidden;
        let sidebar_visible = self.sidebar_visible;
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.entry(
                    if view_mode == ViewMode::Details {
                        "Details ✓"
                    } else {
                        "Details"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel
                                .update(cx, |state, cx| state.set_view_mode(ViewMode::Details, cx));
                        }
                    },
                )
                .entry(
                    if view_mode == ViewMode::Grid {
                        "Large icons ✓"
                    } else {
                        "Large icons"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.set_view_mode(ViewMode::Grid, cx));
                        }
                    },
                )
                .separator()
                .entry(
                    if show_hidden {
                        "Hide hidden files"
                    } else {
                        "Show hidden files"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.toggle_hidden(window, cx));
                        }
                    },
                )
                .entry(
                    if sidebar_visible {
                        "Hide navigation pane"
                    } else {
                        "Show navigation pane"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.toggle_sidebar(cx));
                        }
                    },
                )
            },
            window,
            cx,
        );
    }

    /// The "More" menu of the command bar.
    pub(crate) fn deploy_more_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = !self.selected.is_empty();
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.action_disabled_when(!has_selection, "Rename", Box::new(Rename))
                    .action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                    .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                    .separator()
                    .action_disabled_when(!can_undo, "Undo", Box::new(Undo))
                    .action_disabled_when(!can_redo, "Redo", Box::new(Redo))
                    .separator()
                    .action_disabled_when(!has_selection, "Move to Trash", Box::new(Trash))
                    .action_disabled_when(!has_selection, "Delete Permanently", Box::new(Delete))
            },
            window,
            cx,
        );
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

    /// Sets the view mode.
    pub(crate) fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        cx.notify();
    }

    /// Opens or closes the search box.
    pub(crate) fn toggle_search(&mut self, cx: &mut Context<Self>) {
        match self.search_query.take() {
            Some(_) => {}
            None => self.search_query = Some(String::new()),
        }
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

    /// Appends a keystroke to the search query.
    pub(crate) fn search_input(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(query) = self.search_query.as_mut() {
            query.push_str(key);
        }
        cx.notify();
    }

    /// Removes the last character of the search query.
    pub(crate) fn search_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(query) = self.search_query.as_mut() {
            query.pop();
        }
        cx.notify();
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

    /// Whether the search box fits (OpenXplorer hides it at ≤ 680).
    pub(crate) fn is_search_box_shown(&self) -> bool {
        self.breakpoint_width() >= 640.
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
