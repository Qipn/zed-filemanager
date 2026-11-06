//! The header bar: the sidebar toggle, history buttons, the location slot and
//! the view/new controls. The slot itself lives in `render/location.rs`.
//!
//! It is a single row, laid out like GNOME Files' toolbar
//! (`nautilus-toolbar.blp`):
//!
//! ```text
//! [sidebar] [back] [forward] [ location slot ] [search] [view ▾] [new ▾]
//! ```
//!
//! GNOME keeps the file commands (cut/copy/paste/rename/delete) out of the
//! toolbar: they live in the context menu and on the keyboard. The panel does
//! the same, so the old OpenXplorer command bar is gone.

use gpui::{AnyElement, Context, IntoElement, Window, px};
use theme::ActiveTheme;
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};

use super::location::{render_location_slot, render_search_toggle};
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::views::ViewMode;

/// The header bar of the panel.
pub(super) fn render_header_bar(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let show_view_controls = this.is_header_view_controls_shown();
    let show_new_controls = this.is_header_new_controls_shown();

    h_flex()
        .id("file-manager-header-bar")
        .h(px(metrics::HEADER_BAR))
        .w_full()
        .flex_shrink_0()
        .gap(px(4.))
        // GNOME's header bar pads its content by 6 px.
        .px(px(6.))
        .bg(palette.chrome)
        .border_b_1()
        .border_color(palette.divider)
        // `start`: the sidebar toggle, then the history buttons. GNOME has no
        // "up" or "refresh" buttons (Alt+Up / F5 do that).
        .child(render_sidebar_toggle(this, cx))
        .child(
            IconButton::new("file-manager-back", IconName::ArrowLeft)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Back (Alt+Left)"))
                .disabled(!this.can_go_back())
                .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
        )
        .child(
            IconButton::new("file-manager-forward", IconName::ArrowRight)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Forward (Alt+Right)"))
                .disabled(!this.can_go_forward())
                .on_click(cx.listener(|this, _, window, cx| this.go_forward(window, cx))),
        )
        // `title`: the path bar, the editable location entry or the search
        // field, in one slot like GNOME's `toolbar_switcher` stack.
        .child(render_location_slot(this, window, cx))
        .child(render_search_toggle(this, cx))
        // `end`: the view controls and the new-folder control.
        .when(show_view_controls, |bar| {
            bar.child(render_view_controls(this, cx))
        })
        .when(show_new_controls, |bar| {
            bar.child(render_new_controls(this, cx))
        })
        .into_any_element()
}

/// The sidebar toggle at the start of the header bar
/// (`show_sidebar_button` in GNOME's toolbar).
fn render_sidebar_toggle(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    IconButton::new(
        "file-manager-sidebar-toggle",
        if this.sidebar_visible {
            IconName::ThreadsSidebarLeftOpen
        } else {
            IconName::ThreadsSidebarLeftClosed
        },
    )
    .icon_size(IconSize::Small)
    .tooltip(Tooltip::text("Navigation pane (F9)"))
    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
    .into_any_element()
}

/// The view controls: a split button like GNOME's `Adw.SplitButton`
/// (`nautilus-view-controls.blp`). The main button switches between the details
/// view and the icon grid, the caret opens the view menu.
fn render_view_controls(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let (icon, tooltip) = match this.view_mode {
        ViewMode::Details => (IconName::Table, "List view"),
        ViewMode::Grid => (IconName::Blocks, "Grid view"),
    };

    h_flex()
        .id("file-manager-view-controls")
        .flex_shrink_0()
        .child(
            IconButton::new("file-manager-view-mode", icon)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text(tooltip))
                .on_click(cx.listener(|this, _, _, cx| {
                    let next = match this.view_mode {
                        ViewMode::Details => ViewMode::Grid,
                        ViewMode::Grid => ViewMode::Details,
                    };
                    this.set_view_mode(next, cx);
                })),
        )
        .child(
            IconButton::new("file-manager-view-menu", IconName::ChevronDown)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("View options"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.deploy_view_menu(window.mouse_position(), window, cx);
                })),
        )
        .into_any_element()
}

/// The "new" control: the main button creates a folder, like GNOME's
/// `new_folder_button`; the caret opens the template menu (text/Markdown/CSV/
/// JSON/HTML documents), which GNOME reaches through the templates in its
/// context menu.
fn render_new_controls(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    h_flex()
        .id("file-manager-new-controls")
        .flex_shrink_0()
        .child(
            IconButton::new("file-manager-new-folder", IconName::FolderAdd)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("New folder (Ctrl+Shift+N)"))
                .disabled(this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.create_folder(window, cx))),
        )
        .child(
            IconButton::new("file-manager-new-menu", IconName::ChevronDown)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("New…"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.deploy_new_menu(window.mouse_position(), window, cx);
                })),
        )
        .into_any_element()
}
