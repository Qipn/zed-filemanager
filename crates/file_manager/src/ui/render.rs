//! Rendering for `FileManagerPanel`, laid out like GNOME Files.
//!
//! The band order follows GNOME Files' window and toolbar
//! (`nautilus-window.blp`, `nautilus-toolbar.blp`):
//!
//! ```text
//! header bar    (sidebar toggle, back/forward, location slot, search,
//!                view controls, new folder)
//! body          (sidebar: places + folder tree | folder view)
//! status bar    (count, selection, …, view toggles)
//! ```
//!
//! There is no separate command bar: GNOME keeps the file commands in the
//! context menu and on the keyboard, and the panel does the same. The view
//! layer lives in `views.rs` (details and grid views) and `sidebar.rs` (places
//! and folder tree).

mod body;
mod conflict_banner;
mod header;
mod location;
mod status_bar;

use body::render_body;
use conflict_banner::render_conflict_banner;
use header::render_header_bar;
use status_bar::render_status_bar;

use gpui::{Context, IntoElement, KeyDownEvent, MouseButton, Window, canvas};
use theme::ActiveTheme;
use ui::prelude::*;

use crate::ui::panel::FileManagerPanel;
use crate::{
    CancelInput, ConfirmInput, Copy, Cut, Delete, EditLocation, NewFile, NewFolder, Open,
    OpenAsWorkspace, OpenContextMenu, Paste, Redo, Refresh, Rename, SelectAll, ToggleBookmark,
    ToggleSearch, ToggleSidebar, ToggleViewMode, Trash, Undo,
};

/// Renders the whole panel.
pub(super) fn render(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> impl IntoElement {
    v_flex()
        .size_full()
        // The rows of the folder view must never paint outside the panel.
        .overflow_hidden()
        .key_context("FileManagerPanel")
        .bg(cx.theme().colors().panel_background)
        .track_focus(&this.focus_handle)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                // Clicking anywhere outside an inline editor leaves it. The
                // editors and the address bar stop the propagation of their own
                // clicks, so only clicks on the background reach this handler.
                if this.path_editing {
                    this.cancel_path_edit(window, cx);
                }
                if this.rename_state.is_some() {
                    this.cancel_rename(window, cx);
                }
                window.focus(&this.focus_handle, cx);
            }),
        )
        // The sidebar resizer drags through GPUI's drag-and-drop system, like
        // Zed's dock (`crates/workspace/src/dock.rs`): the handle only starts a
        // drag and the panel resizes on every drag move, so releasing the mouse
        // always ends the resize.
        .on_drag_move::<crate::ui::panel::SidebarResizeDrag>(cx.listener(
            |this, event: &gpui::DragMoveEvent<crate::ui::panel::SidebarResizeDrag>, _, cx| {
                let width = f32::from(event.event.position.x - event.bounds.left());
                this.set_sidebar_width(width, cx);
            },
        ))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            this.handle_key_down(event, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Open, window, cx| {
            this.open_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenAsWorkspace, window, cx| {
            this.open_as_workspace(window, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenContextMenu, window, cx| {
            this.deploy_context_menu(window.mouse_position(), window, cx);
        }))
        .on_action(cx.listener(|this, _: &NewFolder, window, cx| {
            this.create_folder(window, cx);
        }))
        .on_action(cx.listener(|this, _: &NewFile, window, cx| {
            this.create_file(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Refresh, window, cx| {
            this.refresh(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Rename, window, cx| {
            this.rename_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Copy, window, cx| {
            this.copy_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Cut, window, cx| {
            this.cut_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Paste, window, cx| {
            this.paste_clipboard(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Trash, window, cx| {
            this.confirm_remove(true, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Delete, window, cx| {
            this.confirm_remove(false, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Undo, window, cx| {
            this.undo_last(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Redo, window, cx| {
            this.redo_last(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
            this.toggle_sidebar(cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSearch, window, cx| {
            this.toggle_search(window, cx);
        }))
        .on_action(cx.listener(|this, _: &EditLocation, window, cx| {
            this.begin_path_edit(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleBookmark, _, cx| {
            this.toggle_bookmark(cx);
        }))
        .on_action(cx.listener(|this, _: &SelectAll, window, cx| {
            this.select_all(window, cx);
        }))
        // Escape is claimed by the default keymap (`escape` → `menu::Cancel`),
        // which runs before the text fields' own key handlers, so the panel
        // handles it for them.
        .on_action(cx.listener(|this, _: &CancelInput, window, cx| {
            this.cancel_active_input(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ConfirmInput, window, cx| {
            this.confirm_active_input(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleViewMode, _, cx| {
            this.set_view_mode(
                match this.view_mode {
                    crate::ui::views::ViewMode::Details => crate::ui::views::ViewMode::Grid,
                    crate::ui::views::ViewMode::Grid => crate::ui::views::ViewMode::Details,
                },
                cx,
            );
        }))
        .child(render_header_bar(this, window, cx))
        .children(render_conflict_banner(this, cx))
        .child(render_body(this, window, cx))
        .child(render_status_bar(this, cx))
        .children(this.render_context_menu())
        // Measure the panel itself: a Zed dock can be much narrower than the
        // window, so the layout breakpoints follow the panel's own width.
        .child({
            let panel = cx.entity().downgrade();
            let measured = this.content_width;
            canvas(
                move |bounds, window, cx| {
                    let width = f32::from(bounds.size.width);
                    if measured == Some(width.round()) {
                        return;
                    }
                    window.defer(cx, move |_window, cx| {
                        panel
                            .update(cx, |this, cx| this.set_content_width(width, cx))
                            .ok();
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        })
}
