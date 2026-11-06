//! The body of the panel: the sidebar, the resizer, the folder view and the
//! details pane.

use gpui::{AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, Window, div, px};
use ui::prelude::*;

use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::views::render_view;
/// The body: the sidebar and the folder view.
pub(super) fn render_body(
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
