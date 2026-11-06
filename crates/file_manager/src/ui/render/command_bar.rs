//! The command bar: `New`, the file commands, `Sort`, `View` and `More`.

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div, px,
};
use ui::{Color, IconButton, IconName, IconSize, Label, LabelSize, Tooltip, prelude::*};

use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;

pub(super) fn render_command_bar(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let has_selection = !this.selected.is_empty();

    let separator = || div().w(px(1.)).h(px(20.)).mx(px(10.)).bg(palette.divider);

    h_flex()
        .id("file-manager-command-bar")
        .h(px(metrics::COMMAND_BAR))
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(4.))
        .px(px(15.))
        .bg(palette.chrome)
        .border_b_1()
        .border_color(palette.divider)
        // New ▾
        .child(command_button(
            "file-manager-new",
            "New",
            "New".into(),
            this.current_dir.is_none(),
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_new_menu(event.position, window, cx);
            }),
        ))
        .child(separator())
        // Cut, Copy, Paste, Rename, Delete
        .child(
            IconButton::new("file-manager-cut", IconName::Scissors)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Cut (Ctrl+X)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, _, cx| this.cut_selected(cx))),
        )
        .child(
            IconButton::new("file-manager-copy", IconName::Copy)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Copy (Ctrl+C)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
        )
        .child(
            IconButton::new("file-manager-paste", IconName::Download)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Paste files (Ctrl+V)"))
                .disabled(this.clipboard.is_none() || this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.paste_clipboard(window, cx))),
        )
        .child(
            IconButton::new("file-manager-rename", IconName::Pencil)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Rename (F2)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, window, cx| this.rename_selected(window, cx))),
        )
        .child(
            IconButton::new("file-manager-trash", IconName::Trash)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Move to Trash"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, window, cx| this.confirm_remove(true, window, cx))),
        )
        .child(separator())
        // Sort ▾
        .child(command_button(
            "file-manager-sort",
            "Sort",
            format!("Sort: {}", this.sort_mode.label()).into(),
            false,
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_sort_menu(event.position, window, cx);
            }),
        ))
        // View ▾
        .child(command_button(
            "file-manager-view",
            "View",
            "View".into(),
            false,
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_view_menu(event.position, window, cx);
            }),
        ))
        // More ···
        .child(
            IconButton::new("file-manager-more", IconName::Ellipsis)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("More options"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.deploy_more_menu(window.mouse_position(), window, cx);
                })),
        )
        .child(div().flex_1())
        // The details pane toggle, like OpenXplorer's command bar.
        .child(
            IconButton::new("file-manager-details-toggle", IconName::Split)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Details pane"))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_details_pane(cx))),
        )
        // The sidebar toggle lives on the right of the command bar.
        .child(
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
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
        )
        .into_any_element()
}

/// A command bar button with a text label and a chevron, like OpenXplorer's
/// "New ▾", "Sort ▾" and "View ▾".
pub(super) fn command_button(
    id: &'static str,
    label: impl Into<SharedString>,
    tooltip: SharedString,
    disabled: bool,
    palette: &crate::ui::ox_theme::OxPalette,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .tooltip(Tooltip::text(tooltip))
        // A block container would stack the label above the chevron.
        .flex()
        .h(px(34.))
        .px(px(11.))
        .items_center()
        .gap(px(4.))
        .rounded_xs()
        .opacity(if disabled { 0.5 } else { 1. })
        .hover(|style| style.bg(palette.hover))
        .child(Label::new(label).size(LabelSize::Small))
        .child(
            Icon::new(IconName::ChevronDown)
                .size(IconSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}
