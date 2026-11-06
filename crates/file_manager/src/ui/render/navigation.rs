//! The navigation row: history buttons, the shared location slot (breadcrumbs,
//! editable path, or search) and the search toggle.

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div, px,
};
use theme::ActiveTheme;
use ui::{Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip, prelude::*};

use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
pub(super) fn render_navigation_row(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    h_flex()
        .id("file-manager-navigation-row")
        .h(px(metrics::NAV_ROW))
        .w_full()
        .flex_shrink_0()
        .gap(px(8.))
        .px(px(15.))
        .bg(palette.chrome)
        .border_b_1()
        .border_color(palette.divider)
        .child(
            IconButton::new("file-manager-back", IconName::ArrowLeft)
                .icon_size(IconSize::Small)
                .disabled(!this.can_go_back())
                .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
        )
        .child(
            IconButton::new("file-manager-forward", IconName::ArrowRight)
                .icon_size(IconSize::Small)
                .disabled(!this.can_go_forward())
                .on_click(cx.listener(|this, _, window, cx| this.go_forward(window, cx))),
        )
        .child(
            IconButton::new("file-manager-up", IconName::ArrowUp)
                .icon_size(IconSize::Small)
                .disabled(this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.go_up(window, cx))),
        )
        .child(
            IconButton::new("file-manager-refresh", IconName::RotateCw)
                .icon_size(IconSize::Small)
                .disabled(this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
        )
        .child(render_location_slot(this, cx))
        // The magnifier opens the search mode; while searching, the slot
        // itself carries the close button (like GNOME Files). It is always
        // shown: the old width gate hid it in a narrow dock, which made the
        // search unreachable.
        .child(render_search_toggle(this, cx))
        .into_any_element()
}

/// The shared location slot of the navigation row.
///
/// GNOME Files (Nautilus) uses a single field for three modes instead of an
/// address box plus a separate search box: the breadcrumb path, the editable
/// location, and the search query. The panel does the same: the slot shows the
/// search while `search_query` is set, the editable path while `path_edit` is
/// set, and the breadcrumbs otherwise.
pub(super) fn render_location_slot(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    if this.search_query.is_some() {
        render_search_field(this, cx)
    } else {
        render_address_bar(this, cx)
    }
}

/// The search mode of the location slot.
pub(super) fn render_search_field(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());
    let query = this.search_query.clone().unwrap_or_default();

    div()
        .id("file-manager-search-box")
        // `div()` is `display: Block` by default, so `items_center()` alone
        // would be ignored and the icon, label and button would stack
        // vertically. `.flex()` defaults to a row.
        .flex()
        .h(px(32.))
        .flex_1()
        .min_w(px(140.))
        .px(px(8.))
        .items_center()
        .gap(px(6.))
        .rounded_xs()
        .bg(palette.field_bg)
        // Searching is a focused mode, so the field shows the accent edge
        // like Nautilus' search entry.
        .border_1()
        .border_color(palette.accent)
        .child(
            Icon::new(IconName::MagnifyingGlass)
                .size(IconSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .child(
            Label::new(SharedString::from(format!("{query}|")))
                .single_line()
                .truncate()
                .flex_1(),
        )
        .child(
            IconButton::new("file-manager-search-close", IconName::Close)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Close the search (Esc)"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search(cx);
                })),
        )
        .into_any_element()
}

/// The magnifier button that opens the search mode.
pub(super) fn render_search_toggle(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let _ = this;
    IconButton::new("file-manager-search", IconName::MagnifyingGlass)
        .icon_size(IconSize::Small)
        .tooltip(Tooltip::text("Search (Ctrl+F)"))
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_search(cx);
        }))
        .into_any_element()
}

/// The address bar: the breadcrumb path, or the edit field.
pub(super) fn render_address_bar(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());

    if let Some(buffer) = &this.path_edit {
        return div()
            .id("file-manager-address-edit")
            .flex()
            .h(px(32.))
            .flex_1()
            .min_w(px(140.))
            .px(px(11.))
            .items_center()
            .rounded_xs()
            .bg(palette.field_bg)
            .border_1()
            .border_color(palette.border)
            .child(
                Label::new(SharedString::from(format!("{buffer}|")))
                    .single_line()
                    .truncate(),
            )
            .into_any_element();
    }

    let segments = this.breadcrumb_targets();
    let mut crumbs = h_flex().gap(px(2.)).flex_1().min_w(px(0.));

    for (index, (target, name)) in segments.iter().enumerate() {
        let is_last = index + 1 == segments.len();
        if index > 0 {
            // The separator sits in front of every crumb but the first.
            crumbs = crumbs.child(
                Label::new("/")
                    .size(LabelSize::Small)
                    .color(Color::Custom(palette.text_tertiary)),
            );
        }
        let target = target.clone();
        let name = name.clone();
        crumbs =
            crumbs.child(
                h_flex()
                    .id(("file-manager-crumb", index))
                    .h(px(26.))
                    .px(px(9.))
                    .rounded_xs()
                    .hover(|style| style.bg(palette.hover))
                    .child(Label::new(name).single_line().size(LabelSize::Small).color(
                        if is_last {
                            Color::Custom(palette.text)
                        } else {
                            Color::Custom(palette.muted)
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.deploy_breadcrumb_menu(target.clone(), event.position, window, cx);
                        }),
                    ),
            );
    }

    div()
        .id("file-manager-address")
        // See the search box: a block container ignores `items_center()` and
        // stacks the icon, the crumbs and the edit button vertically.
        .flex()
        .h(px(32.))
        .flex_1()
        .min_w(px(140.))
        .px(px(8.))
        .py(px(2.))
        .items_center()
        .rounded_xs()
        .bg(palette.field_bg)
        .border_1()
        .border_color(palette.border)
        .child(
            Icon::new(IconName::Folder)
                .size(IconSize::Small)
                .color(Color::Custom(palette.muted)),
        )
        .child(crumbs)
        .child(
            IconButton::new("file-manager-edit-path", IconName::ChevronDown)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Edit location (Ctrl+L)"))
                .on_click(cx.listener(|this, _, _, cx| this.begin_path_edit(cx))),
        )
        .into_any_element()
}

