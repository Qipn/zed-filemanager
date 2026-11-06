//! Rendering for `FileManagerPanel`, laid out like OpenXplorer.
//!
//! The band order follows the upstream application
//! (`native/docs/ui-spec.md` §1.1):
//!
//! ```text
//! navigation row (back, forward, up, refresh, address bar)
//! command bar   (New | Cut Copy Paste Rename Delete | Sort View More)
//! body          (sidebar: places + folder tree | folder view)
//! status bar    (count, selection, … , view toggles)
//! ```
//!
//! The view layer lives in `views.rs` (details and grid views) and
//! `sidebar.rs` (places and folder tree).

mod command_bar;
mod conflict_banner;
mod navigation;
mod status_bar;

use command_bar::render_command_bar;
use conflict_banner::render_conflict_banner;
use navigation::render_navigation_row;
use status_bar::render_status_bar;

use gpui::{
    AnyElement, Context, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Window, canvas,
    div, px,
};
use theme::ActiveTheme;
use ui::prelude::*;

use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::views::render_view;
use crate::{
    Copy, Cut, Delete, EditLocation, NewFile, NewFolder, Open, OpenContextMenu, Paste, Redo,
    Refresh, Rename, ToggleSearch, ToggleSidebar, ToggleViewMode, Trash, Undo,
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
        .on_action(cx.listener(|this, _: &Copy, _, cx| {
            this.copy_selected(cx);
        }))
        .on_action(cx.listener(|this, _: &Cut, _, cx| {
            this.cut_selected(cx);
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
        .on_action(cx.listener(|this, _: &ToggleSearch, _, cx| {
            this.toggle_search(cx);
        }))
        .on_action(cx.listener(|this, _: &EditLocation, _, cx| {
            this.begin_path_edit(cx);
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
        .child(render_navigation_row(this, window, cx))
        .child(render_command_bar(this, window, cx))
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

/// The body: the sidebar and the folder view.
fn render_body(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    let content = v_flex()
        .id("file-manager-content")
        .flex_1()
        // `min_h(0)` lets the column shrink below its content height, which is
        // what makes the list inside scroll instead of overflowing.
        .min_h(px(0.))
        .min_w(px(0.))
        // Fill the body height: the body row centres its children by default
        // (`ui::h_flex()` sets `items_center()`), which would float a
        // content-sized list in the middle of the panel.
        .h_full()
        .overflow_hidden()
        .child(
            v_flex()
                .id("file-manager-entries-wrapper")
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                // Like Zed's project_panel, the whole pane lights up with
                // `drop_target_background` when files are dragged over it.
                .drag_over::<crate::ui::panel::DraggedPaths>(|style, _, _, cx| {
                    style.bg(cx.theme().colors().drop_target_background)
                })
                .drag_over::<gpui::ExternalPaths>(|style, _, _, cx| {
                    style.bg(cx.theme().colors().drop_target_background)
                })
                .on_drag_move::<crate::ui::panel::DraggedPaths>(cx.listener(
                    move |this,
                          event: &gpui::DragMoveEvent<crate::ui::panel::DraggedPaths>,
                          _,
                          cx| {
                        let hovered = event.bounds.contains(&event.event.position);
                        if hovered && this.drag_target.is_none() {
                            if let Some(dir) = this.current_dir.clone() {
                                this.drag_target = Some(dir);
                                cx.notify();
                            }
                        }
                    },
                ))
                .on_drag_move::<gpui::ExternalPaths>(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<gpui::ExternalPaths>, _, cx| {
                        let hovered = event.bounds.contains(&event.event.position);
                        if hovered && this.drag_target.is_none() {
                            if let Some(dir) = this.current_dir.clone() {
                                this.drag_target = Some(dir);
                                cx.notify();
                            }
                        }
                    },
                ))
                .on_drop(cx.listener(
                    move |this, dragged: &crate::ui::panel::DraggedPaths, window, cx| {
                        if let Some(dir) = this.current_dir.clone() {
                            this.drop_dragged(
                                crate::ui::panel::DraggedPaths(dragged.0.clone()),
                                dir,
                                window,
                                cx,
                            );
                        }
                    },
                ))
                .on_drop(
                    cx.listener(move |this, external: &gpui::ExternalPaths, window, cx| {
                        if let Some(dir) = this.current_dir.clone() {
                            let paths: Vec<std::path::PathBuf> =
                                external.0.iter().cloned().collect();
                            this.drop_external(paths, dir, window, cx);
                        }
                    }),
                )
                .child(render_view(this, window, cx)),
        );

    if !this.is_sidebar_shown() {
        return content.into_any_element();
    }

    // `items_stretch`, not the `h_flex()` default: the sidebar, the folder view
    // and the details pane all have to span the full body height instead of
    // being centred vertically.
    h_flex()
        .items_stretch()
        .flex_1()
        .min_h(px(0.))
        .child(crate::ui::sidebar::render_sidebar(this, window, cx))
        // The resizer between the sidebar and the content. It starts a drag
        // instead of tracking the mouse itself, so GPUI ends the resize when
        // the button is released (see Zed's dock resize handle).
        .child(
            div()
                .id("file-manager-sidebar-resizer")
                .w(px(metrics::SIDEBAR_RESIZER))
                .h_full()
                .flex_shrink_0()
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                .hover(|style| style.bg(palette.drop_target))
                .on_drag(crate::ui::panel::SidebarResizeDrag, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| crate::ui::panel::SidebarResizeDrag)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                    }),
                )
                // Double click restores OpenXplorer's default width.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                        if event.click_count == 2 {
                            this.reset_sidebar_width(cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .occlude(),
        )
        .child(content)
        .when(this.is_details_pane_shown(), |body| {
            body.children(crate::ui::details_pane::render_details_pane(
                this, window, cx,
            ))
        })
        .into_any_element()
}
