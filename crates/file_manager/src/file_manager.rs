//! A file manager dock panel.
//!
//! Lists one directory at a time, navigates into directories, opens files in
//! Zed, creates folders and files, renames, copies, cuts, pastes, and moves
//! items to the Trash or deletes them. It uses Zed's `Project`/`Fs` APIs so it
//! works with the same local and remote worktrees as the rest of the editor.
//!
//! Module layout, split like the rest of the workspace into a model layer and
//! a view layer:
//!
//! - [`model`]: data and rules, no UI entities.
//!   - [`model::entries`]: directory listing (`DirEntry`, `SortMode`).
//!   - [`model::search`]: recursive search over the current folder.
//!   - [`model::tree`]: the folder tree nodes.
//!   - [`model::clipboard`]: the panel clipboard and file-operation helpers.
//!   - [`model::conflict`]: name-conflict resolution for paste and drag.
//!   - [`model::undo`]: the undo/redo history.
//!   - [`model::xdg`]: the home, config and quick-access directories.
//!   - [`model::bookmarks`]: the folders pinned to the sidebar.
//! - [`ui`]: the `gpui` layer.
//!   - [`ui::panel`]: the `FileManagerPanel` entity and its `Panel` impl.
//!   - [`ui::panel::persistence`]: the panel state kept between sessions.
//!   - [`ui::panel::bookmarks`]: pinning and unpinning sidebar folders.
//!   - [`ui::render`]: the window layout (GNOME Files header bar, body, status bar).
//!   - [`ui::views`]: the details view and the icon grid.
//!   - [`ui::views::icons`]: file-type and folder icons from the icon theme.
//!   - [`ui::sidebar`]: the places list and the folder tree.
//!   - [`ui::details_pane`]: the details pane on the right.
//!   - [`ui::places`]: the "Places" entries of the sidebar.
//!   - [`ui::ox_theme`]: the OpenXplorer design tokens.

mod model;
mod ui;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gpui::{App, Context, Window, actions};
use workspace::Workspace;

pub use ui::FileManagerPanel;

actions!(
    file_manager,
    [
        /// Toggles the file manager panel.
        Toggle,
        /// Toggles focus on the file manager panel.
        ToggleFocus,
        /// Opens the selected entry.
        Open,
        /// Opens the selected folder (or the current folder) as a new Zed
        /// workspace, in a new window.
        OpenAsWorkspace,
        /// Opens the selected folder (or the current folder) as a workspace in
        /// the current window, switching to it and keeping the previous
        /// workspace available in the window's workspace group.
        OpenInThisWindow,
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
        /// Puts the selected entry on the clipboard in hard-link mode: pasting
        /// creates hard links instead of duplicating the data.
        CopyHardLink,
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
        /// Opens or closes the search mode in the location slot.
        ToggleSearch,
        /// Turns the location slot into the editable path field.
        EditLocation,
        /// Pins the current folder in the sidebar, or unpins it.
        ToggleBookmark,
        /// Selects every row of the folder view.
        SelectAll,
        /// Cancels the active inline input (location entry, rename) or leaves the
        /// search mode.
        CancelInput,
        /// Confirms the active inline input, or opens the selection when none is
        /// being edited.
        ConfirmInput,
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
