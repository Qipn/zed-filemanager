//! A file manager dock panel.
//!
//! Lists one directory at a time, navigates into directories, opens files in
//! Zed, creates folders and files, renames, copies, cuts, pastes, and moves
//! items to the Trash or deletes them. It uses Zed's `Project`/`Fs` APIs so it
//! works with the same local and remote worktrees as the rest of the editor.
//!
//! Module layout:
//! - [`panel`]: the `FileManagerPanel` entity and its `Panel` implementation.
//! - [`render`]: the view layer, laid out like OpenXplorer.
//! - [`views`]: the details view and the icon grid.
//! - [`sidebar`]: the places list and the folder tree.
//! - [`entries`]: directory listing.
//! - [`clipboard`]: the panel clipboard and file-operation helpers.
//! - [`conflict`]: name-conflict resolution for paste, cut and drag & drop.
//! - [`details_pane`]: the details pane on the right of the folder view.
//! - [`places`]: the "Places" list of the sidebar.
//! - [`tree`]: the folder tree of the sidebar.
//! - [`ox_theme`]: the OpenXplorer design tokens (metrics and palette).
//! - [`undo`]: the undo/redo history.

mod clipboard;
mod conflict;
mod details_pane;
mod entries;
mod ox_theme;
mod panel;
mod places;
mod render;
mod sidebar;
mod tree;
mod undo;
mod views;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gpui::{App, Context, Window, actions};
use workspace::Workspace;

pub use panel::FileManagerPanel;

actions!(
    file_manager,
    [
        /// Toggles the file manager panel.
        Toggle,
        /// Toggles focus on the file manager panel.
        ToggleFocus,
        /// Opens the selected entry.
        Open,
        /// Opens the context menu for the selected entry.
        OpenContextMenu,
        /// Creates a new folder in the current directory.
        NewFolder,
        /// Creates a new file in the current directory.
        NewFile,
        /// Refreshes the current directory.
        Refresh,
        /// Renames the selected entry.
        Rename,
        /// Copies the selected entry to the panel clipboard.
        Copy,
        /// Cuts the selected entry to the panel clipboard.
        Cut,
        /// Pastes from the panel clipboard into the current directory.
        Paste,
        /// Moves the selected entry to the Trash.
        Trash,
        /// Deletes the selected entry permanently.
        Delete,
        /// Undoes the last file operation.
        Undo,
        /// Redoes the last undone file operation.
        Redo,
        /// Shows or hides the sidebar (the places list and the folder tree).
        ToggleSidebar,
        /// Switches between the details view and the icon grid.
        ToggleViewMode,
    ]
);

/// Registers the panel's actions.
pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<FileManagerPanel>(window, cx);
        });
        workspace.register_action(|workspace, _: &Toggle, window, cx| {
            if !workspace.toggle_panel_focus::<FileManagerPanel>(window, cx) {
                workspace.close_panel::<FileManagerPanel>(window, cx);
            }
        });
    })
    .detach();
}

/// Reveals `path` in the file manager dock, opening and focusing it.
///
/// Returns `false` when the workspace has no file manager panel; callers can
/// then fall back to the platform file manager.
pub fn reveal_in_file_manager(
    workspace: &mut Workspace,
    path: PathBuf,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> bool {
    let Some(panel) = workspace.panel::<FileManagerPanel>(cx) else {
        return false;
    };

    panel.update(cx, |panel, cx| panel.reveal_path(path, window, cx));
    workspace.open_panel::<FileManagerPanel>(window, cx);
    let _focused = workspace.focus_panel::<FileManagerPanel>(window, cx);
    true
}
