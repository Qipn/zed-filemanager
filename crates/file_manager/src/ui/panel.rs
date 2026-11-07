//! The `FileManagerPanel` entity: state, navigation, file operations and the
//! `Panel` trait implementation. Rendering lives in `render.rs`, the menu
//! builders in `panel/menus.rs`.

mod batch_rename;
mod bookmarks;
pub(crate) mod columns;
mod file_ops;
mod history;
mod layout;
mod location;
mod menus;
mod navigation;
mod persistence;
pub(crate) mod preview;
mod rename;
mod scroll;
mod search;
mod selection;
mod sidebar_state;
mod sort;
mod transfer;
mod view_state;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use fs::Fs;
use gpui::{
    Action, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Pixels, Point, Render, ScrollHandle, SharedString, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, px,
};
use project::Project;
use ui::prelude::*;
use util::ResultExt;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::ToggleFocus;
use crate::model::bookmarks::Bookmark;
use crate::model::clipboard::Clipboard;
use crate::model::conflict::{ConflictPolicy, TransferKind};
use crate::model::entries::{DirEntry, SortMode, read_entries};
use crate::model::tree::TreeNode;
use crate::model::undo::UndoManager;
use crate::ui::panel::persistence::PersistedState;
use crate::ui::places::PlaceEntry;
use crate::ui::render as render_panel;
use crate::ui::search_field::{SearchField, SearchFieldEvent};
use crate::ui::views::ViewMode;

/// A transfer waiting for the user to decide how name conflicts are resolved.
///
/// Each conflict is answered one at a time, so the prompt can offer a name to
/// type instead of the suggested one (`nautilus-file-conflict-dialog.c` does the
/// same), and an answer can be applied to every remaining conflict.
pub(crate) struct PendingTransfer {
    /// The paths being transferred.
    pub(crate) sources: Vec<PathBuf>,
    /// The planned destinations, parallel to `sources`.
    pub(crate) targets: Vec<PathBuf>,
    /// How the transfer writes its targets (move / copy / hard link).
    pub(crate) kind: TransferKind,
    /// Indices into `sources` whose target already exists.
    pub(crate) conflicts: Vec<usize>,
    /// `true` when the transfer came from the clipboard's *cut* list, so the
    /// clipboard can be cleared afterwards.
    pub(crate) from_cut: bool,
    /// The conflict the prompt is currently asking about: an index into
    /// `conflicts`.
    pub(crate) current: usize,
    /// Answers already given, parallel to `conflicts`.
    pub(crate) decisions: Vec<Option<ConflictDecision>>,
}

/// How one conflict was resolved.
#[derive(Clone)]
pub(crate) struct ConflictDecision {
    pub(crate) policy: ConflictPolicy,
    /// The name the user typed instead of the suggested one.
    pub(crate) target: Option<PathBuf>,
}

/// A transfer in flight, shown as a progress row under the header bar.
///
/// The values are updated once per transferred item: a directory copy reports
/// progress per top-level entry (Nautilus counts each file inside a directory,
/// which would need its own reporting walk; per entry keeps the common paste
/// responsive and cancellable).
pub(crate) struct TransferProgress {
    /// How many entries the transfer covers.
    pub(crate) total: usize,
    /// How many have been processed.
    pub(crate) done: usize,
    /// The entry being processed, for the status text.
    pub(crate) current: SharedString,
    /// `true` for a copy, `false` for a move.
    pub(crate) kind: TransferKind,
    /// Set by the Cancel button; the loop stops after the current entry.
    pub(crate) cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    /// Loading the children of the tree root. Kept apart from `_loading_task`
    /// (the folder view): both used to share one slot, so navigating cancelled
    /// the tree load and the tree stayed empty.
    pub(crate) _tree_loading_task: Option<Task<()>>,
    /// The task that expands the tree down to the current directory
    /// (`reveal_in_tree`); kept apart from the directory load so navigating does
    /// not cancel the reveal and vice versa.
    pub(crate) _tree_task: Option<Task<()>>,
    /// The task of the running search.
    ///
    /// Kept apart from `_loading_task`: a new search must not drop (and thus
    /// cancel) an in-flight directory load, and navigating must not cancel the
    /// search either.
    pub(crate) _search_task: Option<Task<()>>,
    pub(crate) _operation_task: Option<Task<()>>,
    /// The pending write of the panel state
    /// (`ui::panel::persistence`). Replaced on every change, so the write only
    /// happens once a burst of changes has settled.
    pub(crate) _persist_task: Option<Task<()>>,
    pub(crate) error: Option<SharedString>,
    pub(crate) clipboard: Option<Clipboard>,
    /// The entry being renamed inline, if any. The typed text lives in the
    /// panel's [`crate::ui::search_field::SearchField`] entity, so the inline
    /// editor gets a real caret, selection and input-method support (the rename
    /// row renders that entity in place of the name).
    pub(crate) rename_state: Option<PathBuf>,
    pub(crate) context_menu: Option<DeployedContextMenu>,
    pub(crate) undo_manager: UndoManager,
    pub(crate) show_hidden: bool,
    pub(crate) sort_mode: SortMode,
    /// Whether the location slot is in path-edit mode. The text itself lives in
    /// `path_field`; this is only the mode flag.
    pub(crate) path_editing: bool,
    /// The entry currently being hovered by a drag, for drop highlight.
    pub(crate) drag_target: Option<PathBuf>,
    /// A transfer waiting for conflict resolution.
    pub(crate) pending_transfer: Option<PendingTransfer>,
    /// The transfer currently running, for the progress row.
    pub(crate) transfer_progress: Option<TransferProgress>,
    /// The name offered by the conflict prompt; editable, so the user can rename
    /// the incoming entry instead of replacing or skipping it.
    pub(crate) conflict_field: Entity<SearchField>,
    /// Whether the next conflict answer also applies to every remaining one.
    pub(crate) conflict_apply_to_all: bool,
    /// Whether the batch rename prompt is open.
    pub(crate) batch_rename_open: bool,
    /// The pattern typed in the batch rename prompt.
    pub(crate) batch_field: Entity<SearchField>,
    /// The entry the details pane is previewing (`Space`), if any.
    pub(crate) preview_path: Option<PathBuf>,
    /// The text snippet loaded for that entry, when it is a text file.
    pub(crate) preview_text: Option<SharedString>,
    /// Which view shows the contents of the current directory.
    pub(crate) view_mode: ViewMode,
    /// Whether the sidebar (places + folder tree) is shown.
    pub(crate) sidebar_visible: bool,
    /// Whether the details pane on the right is shown.
    pub(crate) details_pane_visible: bool,
    /// The search query of the search box; `None` when the box is closed.
    pub(crate) search_query: Option<String>,
    /// What the search looks at: scope, file kind, date and content filters.
    pub(crate) search_options: crate::model::search::SearchOptions,
    /// The text entry of the search mode. It owns the text, the caret and the
    /// selection; the panel keeps `search_query` in sync so the rest of the
    /// panel can keep working with a plain string.
    pub(crate) search_field: Entity<SearchField>,
    /// The inline rename editor. It is the same kind of entity as the search
    /// entry, so renaming gets a caret, selection and input-method support.
    pub(crate) rename_field: Entity<SearchField>,
    /// The editable location entry. Same entity again: the path bar gets a real
    /// caret, selection and input-method support.
    pub(crate) path_field: Entity<SearchField>,
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
    /// Share of the sidebar height given to the places list, between 0 and 1;
    /// the folder tree gets the rest.
    pub(crate) sidebar_split: f32,
    /// The places list of the sidebar.
    pub(crate) places: Vec<PlaceEntry>,
    /// The folders the user pinned; they are also part of `places`.
    pub(crate) bookmarks: Vec<Bookmark>,
    /// The root of the folder tree.
    pub(crate) tree_root: Option<PathBuf>,
    /// The direct children of the tree root.
    pub(crate) tree_nodes: Vec<TreeNode>,
    /// Scroll handle of the folder tree, so revealing the current directory can
    /// scroll it into view (`scroll to reveal`).
    pub(crate) tree_scroll: ScrollHandle,
    /// Scroll handle of the folder view (details list and icon grid), so a file
    /// operation can scroll the entry it just created into view
    /// (`ui::panel::scroll`).
    pub(crate) content_scroll: UniformListScrollHandle,
    /// The entry a file operation wants scrolled into view. A create/rename
    /// hands this over and the next directory load consumes it, because the row
    /// only exists once the reload has landed.
    pub(crate) pending_reveal: Option<PathBuf>,
    /// The directory the tree was last revealed for. Revealing only runs when
    /// the current directory changes, so a manual collapse is not undone on
    /// every render.
    pub(crate) last_revealed: Option<PathBuf>,
    /// The width the panel was last laid out at, used for the responsive
    /// breakpoints. `None` until the first frame.
    pub(crate) content_width: Option<f32>,
    /// Total and available bytes of the drive that holds the current folder, for
    /// the status bar's capacity bar (`None` while unknown or remote).
    pub(crate) disk_space: Option<(u64, u64)>,
    /// Widths of the details view's `Date modified` / `Type` / `Size` columns.
    pub(crate) column_widths: [f32; 3],
    /// Whether each of those columns is shown (the user's choice; the responsive
    /// breakpoints additionally drop the ones a narrow dock cannot fit).
    pub(crate) columns_shown: [bool; 3],
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
        // The sidebar geometry and the view preferences are restored from the
        // previous session (`ui::panel::persistence`), except for the directory:
        // that always follows the project, so a panel opened in another project
        // cannot start in a folder that is not part of it.
        let state = PersistedState::load(cx);
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
            _tree_loading_task: None,
            _tree_task: None,
            _search_task: None,
            _operation_task: None,
            _persist_task: None,
            error: None,
            clipboard: None,
            rename_state: None,
            context_menu: None,
            undo_manager: UndoManager::new(),
            show_hidden: state.show_hidden,
            sort_mode: state.sort_mode,
            path_editing: false,
            drag_target: None,
            pending_transfer: None,
            transfer_progress: None,
            conflict_field: cx.new(|cx| SearchField::new("file-manager-conflict-input", cx)),
            conflict_apply_to_all: false,
            batch_rename_open: false,
            batch_field: cx.new(|cx| SearchField::new("file-manager-batch-input", cx)),
            preview_path: None,
            preview_text: None,
            view_mode: state.view_mode,
            sidebar_visible: state.sidebar_visible,
            details_pane_visible: state.details_pane_visible,
            search_query: None,
            search_options: crate::model::search::SearchOptions::default(),
            search_field: cx.new(|cx| SearchField::new("file-manager-search-input", cx)),
            rename_field: cx.new(|cx| SearchField::new("file-manager-rename-input", cx)),
            path_field: cx.new(|cx| SearchField::new("file-manager-path-input", cx)),
            search_results: None,
            search_running: false,
            search_generation: 0,
            expanded_dirs: std::collections::HashMap::new(),
            icon_size: state.icon_size,
            sidebar_width: state.sidebar_width,
            sidebar_split: state.sidebar_split,
            places: Vec::new(),
            bookmarks: Vec::new(),
            tree_root: None,
            tree_nodes: Vec::new(),
            tree_scroll: ScrollHandle::new(),
            content_scroll: UniformListScrollHandle::new(),
            pending_reveal: None,
            last_revealed: None,
            content_width: None,
            disk_space: None,
            column_widths: state.column_widths,
            columns_shown: state.columns_shown,
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
            // The search entry owns its text; the panel mirrors it and restarts
            // the search on every change, like GNOME Files' `changed` handler
            // (`nautilus-query-editor.blp`).
            let search_field = panel.search_field.clone();
            cx.subscribe_in(
                &search_field,
                window,
                |this, field, event: &SearchFieldEvent, window, cx| match event {
                    SearchFieldEvent::Change => {
                        let text = field.read(cx).content().to_string();
                        this.search_query = Some(text);
                        this.start_search(window, cx);
                    }
                    // GNOME's query editor emits `activated` on Enter and the
                    // window slot opens the selection
                    // (`nautilus-window-slot.c::query_editor_activated_callback`).
                    // With nothing selected the focus goes back to the list so
                    // Enter/arrow keys work from there.
                    SearchFieldEvent::Submit => {
                        if this.selected_entry().is_some() {
                            this.open_selected(window, cx);
                        } else {
                            window.focus(&this.focus_handle, cx);
                        }
                    }
                    SearchFieldEvent::Dismiss => {
                        this.toggle_search(window, cx);
                    }
                },
            )
            .detach();

            // The inline rename and the location entry share the same event
            // shape: Enter confirms, Escape cancels (Escape and Enter never
            // reach the fields directly — see `confirm_active_input`).
            panel.subscribe_inline_editor(
                &panel.rename_field.clone(),
                window,
                cx,
                FileManagerPanel::confirm_rename,
                FileManagerPanel::cancel_rename,
            );
            panel.subscribe_inline_editor(
                &panel.path_field.clone(),
                window,
                cx,
                FileManagerPanel::confirm_path_edit,
                FileManagerPanel::cancel_path_edit,
            );
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

    /// Subscribes an inline editor whose only event handling is "Enter confirms,
    /// Escape cancels".
    ///
    /// The search entry has its own subscription because it also restarts the
    /// search on every change.
    fn subscribe_inline_editor(
        &self,
        field: &Entity<SearchField>,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_submit: fn(&mut FileManagerPanel, &mut Window, &mut Context<FileManagerPanel>),
        on_dismiss: fn(&mut FileManagerPanel, &mut Window, &mut Context<FileManagerPanel>),
    ) {
        cx.subscribe_in(
            field,
            window,
            move |this, _field, event: &SearchFieldEvent, window, cx| match event {
                SearchFieldEvent::Submit => on_submit(this, window, cx),
                SearchFieldEvent::Dismiss => on_dismiss(this, window, cx),
                SearchFieldEvent::Change => {}
            },
        )
        .detach();
    }

    fn load_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.current_dir.clone() else {
            return;
        };

        // The capacity bar of the status bar describes this drive. `statvfs` is a
        // single cheap syscall, so it is read here rather than on every frame.
        self.disk_space = crate::model::fs_space::space_for(&directory);

        self.loading_generation = self.loading_generation.wrapping_add(1);
        let generation = self.loading_generation;
        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let sort_mode = self.sort_mode;

        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read_entries(fs, directory, show_hidden, sort_mode).await;
            this.update_in(cx, |this, window, cx| {
                if this.loading_generation != generation {
                    return;
                }
                match result {
                    Ok(entries) => {
                        this.entries = entries;
                        this.error = None;
                        // The row of an entry a file operation just produced only
                        // exists now, so this is the first moment it can be
                        // scrolled into view.
                        this.reveal_pending_entry(window, cx);
                    }
                    Err(error) => {
                        this.entries.clear();
                        this.error = Some(format!("{error}").into());
                    }
                }
                // A freshly created entry only exists as a row once the
                // directory is loaded, so the inline rename editor takes the
                // focus here rather than when the rename starts.
                this.focus_rename_field(window, cx);
                cx.notify();
            })
            .log_err();
        }));
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
