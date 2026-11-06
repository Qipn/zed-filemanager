//! The shared location slot: the breadcrumbs, the editable path and the search
//! field.
//!
//! GNOME Files uses one field for three modes instead of an address box plus a
//! separate search box (`nautilus-toolbar.blp` stacks `pathbar` / `location` /
//! `search`), and the panel does the same: the slot shows the search while it is
//! open, the editable path while `path_editing` is on, and the breadcrumbs
//! otherwise.

use gpui::{
    AnyElement, Context, Focusable, IntoElement, MouseButton, MouseDownEvent, Window, div, px,
};
use std::path::PathBuf;
use theme::ActiveTheme;
use ui::{Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip, prelude::*};

use crate::ui::ox_theme::palette;
use crate::ui::panel::FileManagerPanel;
pub(super) fn render_location_slot(
    this: &FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    if this.search_query.is_some() {
        render_search_field(this, cx)
    } else {
        render_address_bar(this, window, cx)
    }
}

/// The search mode of the location slot.
///
/// Structured like GNOME's `NautilusQueryEditor`
/// (`nautilus-query-editor.blp`): a prefix icon, the real text entry and a
/// clear button. The entry is a [`SearchField`] entity, so caret movement,
/// selection, clipboard and input methods all work.
pub(super) fn render_search_field(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    // GNOME hides the clear icon while the entry is empty
    // (`gtk_widget_set_child_visible (editor->clear_icon, !is_empty)`).
    let has_text = !this.search_field.read(cx).content().is_empty();

    div()
        .id("file-manager-search-box")
        // `div()` is `display: Block` by default, so `items_center()` alone
        // would be ignored and the icon, field and button would stack
        // vertically. `.flex()` defaults to a row.
        .flex()
        .h(px(32.))
        .flex_1()
        .min_w(px(120.))
        .px(px(8.))
        .items_center()
        .gap(px(6.))
        .rounded_xs()
        .bg(palette.field_bg)
        // Searching is a focused mode, so the field shows the accent edge like
        // Nautilus' search entry.
        .border_1()
        .border_color(palette.accent)
        // Clicking anywhere in the entry — the prefix icon or the empty area
        // next to the text — focuses the input, like GNOME's search entry.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                let focus = this.search_field.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }),
        )
        .child(
            Icon::new(IconName::MagnifyingGlass)
                .size(IconSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .child(this.search_field.clone())
        .when(has_text, |bar| {
            bar.child(
                IconButton::new("file-manager-search-clear", IconName::Close)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Clear"))
                    // Only clears the text, like GNOME's `edit-clear-symbolic`
                    // icon; leaving the search mode is the search toggle in the
                    // header bar (or Escape).
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.search_field
                            .update(cx, |field, cx| field.clear(window, cx));
                    })),
            )
        })
        .into_any_element()
}

/// The magnifier button that opens the search mode (`search_button` in
/// GNOME's toolbar).
pub(super) fn render_search_toggle(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let _ = this;
    IconButton::new("file-manager-search", IconName::MagnifyingGlass)
        .icon_size(IconSize::Small)
        .tooltip(Tooltip::text("Search (Ctrl+F)"))
        .on_click(cx.listener(|this, _, window, cx| {
            this.toggle_search(window, cx);
        }))
        .into_any_element()
}

/// The address bar: the breadcrumb path, or the edit field.
///
/// Clicking anywhere in the bar — not only a crumb or the trailing chevron —
/// enters the edit mode, the way Explorer and GNOME's location entry behave.
/// Crumbs stop the propagation so they open their jump menu instead.
pub(super) fn render_address_bar(
    this: &FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let _ = window;
    let palette = palette(cx.theme());

    if this.path_editing {
        return div()
            .id("file-manager-address-edit")
            .flex()
            .h(px(32.))
            .flex_1()
            .min_w(px(120.))
            .px(px(8.))
            .items_center()
            .rounded_xs()
            .bg(palette.field_bg)
            .border_1()
            // The accent edge is the same signal the search entry uses for
            // "this field has the focus and takes text".
            .border_color(palette.accent)
            // Clicking the entry focuses the input, like the search box.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.focus_path_field(window, cx);
                }),
            )
            .child(this.path_field.clone())
            .into_any_element();
    }

    let segments = this.breadcrumb_targets();
    // The root is drawn as the leading button, not as a `/` crumb: the
    // separators below are then a plain join (no `//` special case) and the icon
    // becomes the "jump to /" target, the way Nautilus renders its root button.
    let root = segments.first().cloned();
    let rest: &[(PathBuf, SharedString)] = segments.get(1..).unwrap_or(&[]);

    // Keep the crumbs that matter in a narrow dock: the earliest ones are
    // dropped in favour of the last few (the current folder and its parents),
    // otherwise every label is squeezed and only the tail stays clickable.
    // GNOME's path bar ellipsizes the middle the same way.
    let max_segments = if this.breakpoint_width() >= 760. {
        usize::MAX
    } else if this.breakpoint_width() >= 480. {
        4
    } else {
        2
    };
    let skipped = rest.len().saturating_sub(max_segments);
    let visible = &rest[skipped..];

    // The crumbs must not shrink: a squeezed label is both unreadable and hard
    // to hit.
    let mut crumbs = h_flex()
        .gap(px(2.))
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden();

    if visible.is_empty() {
        // The current folder *is* the root, so there is no segment to draw.
        crumbs = crumbs.child(
            Label::new("/")
                .size(LabelSize::Small)
                .color(Color::Custom(palette.text)),
        );
    }

    if skipped > 0 {
        // The segments that did not fit stay reachable through this menu, so a
        // narrow dock never hides an ancestor (the old version was a static
        // label, which made clicking the "username" look like a dead spot).
        let hidden: Vec<(PathBuf, gpui::SharedString)> = rest[..skipped].to_vec();
        let tooltip = hidden
            .last()
            .map(|(path, _)| path.display().to_string())
            .unwrap_or_default();
        crumbs = crumbs.child(
            h_flex()
                .id("file-manager-crumb-ellipsis")
                .h(px(26.))
                .px(px(9.))
                .flex_shrink_0()
                .rounded_xs()
                .occlude()
                .hover(|style| style.bg(palette.hover))
                .tooltip(Tooltip::text(tooltip))
                .child(
                    Label::new("…")
                        .size(LabelSize::Small)
                        .color(Color::Custom(palette.text_tertiary)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.deploy_ancestor_menu(hidden.clone(), event.position, window, cx);
                    }),
                ),
        );
    }

    for (offset, (target, name)) in visible.iter().enumerate() {
        let index = skipped + offset;
        let is_last = offset + 1 == visible.len();
        // A plain join: the root is no longer one of the crumbs, so the slash is
        // inserted between every pair and after the ellipsis.
        if offset > 0 || skipped > 0 {
            crumbs = crumbs.child(
                Label::new("/")
                    .size(LabelSize::Small)
                    .color(Color::Custom(palette.text_tertiary)),
            );
        }
        let target = target.clone();
        let name = name.clone();
        let navigate_target = target.clone();
        let menu_target = target.clone();

        let crumb = h_flex()
            .id(("file-manager-crumb", index))
            .h(px(26.))
            .px(px(9.))
            .flex_shrink_0()
            .rounded_xs()
            .child(
                Label::new(name)
                    .single_line()
                    .size(LabelSize::Small)
                    .color(if is_last {
                        Color::Custom(palette.text)
                    } else {
                        Color::Custom(palette.muted)
                    }),
            );

        // Nautilus wires the current-folder button to `toolbar.edit-location`
        // instead of navigation (`nautilus-pathbar.c::make_button_data`):
        // clicking the folder you are already in switches the path bar to the
        // editable location entry, and only the ancestors navigate.
        let crumb = if is_last {
            crumb.hover(|style| style.bg(palette.hover)).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.begin_path_edit(window, cx);
                }),
            )
        } else {
            crumb
                // `occlude` makes the crumb an event target of its own, so a
                // click cannot fall through to the address bar container by
                // accident.
                .occlude()
                .hover(|style| style.bg(palette.hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.navigate_to(navigate_target.clone(), window, cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        // Right click keeps the lateral jump menu: "go to this
                        // folder" plus its siblings.
                        cx.stop_propagation();
                        this.deploy_breadcrumb_menu(
                            menu_target.clone(),
                            event.position,
                            window,
                            cx,
                        );
                    }),
                )
        };

        crumbs = crumbs.child(crumb);
    }

    div()
        .id("file-manager-address")
        // See the search box: a block container ignores `items_center()` and
        // stacks the icon, the crumbs and the edit button vertically.
        .flex()
        .h(px(32.))
        .flex_1()
        .min_w(px(120.))
        .px(px(8.))
        .py(px(2.))
        .items_center()
        .rounded_xs()
        .bg(palette.field_bg)
        .border_1()
        .border_color(palette.border)
        // Clicking the bar itself (the icon, the gaps between crumbs, the space
        // after the last one) enters the editable path. The tooltip carries the
        // discoverability the old trailing chevron used to provide; the chevron
        // itself was removed because it duplicated this and looked like a
        // dropdown.
        .tooltip(Tooltip::text("Click to edit the path (Ctrl+L)"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.begin_path_edit(window, cx);
            }),
        )
        .child({
            // The leading folder is the root button: clicking it jumps to `/`.
            // Nautilus draws a drive icon there instead of a `/` crumb.
            let root_target = root.as_ref().map(|(path, _)| path.clone());
            let root_label = root
                .as_ref()
                .map(|(path, _)| path.display().to_string())
                .unwrap_or_default();
            h_flex()
                .id("file-manager-crumb-root")
                .h(px(26.))
                .px(px(6.))
                .flex_shrink_0()
                .rounded_xs()
                .occlude()
                .hover(|style| style.bg(palette.hover))
                .tooltip(Tooltip::text(root_label))
                .child(
                    Icon::new(IconName::Folder)
                        .size(IconSize::Small)
                        .color(Color::Custom(palette.muted)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(target) = root_target.clone() {
                            this.navigate_to(target, window, cx);
                        }
                    }),
                )
        })
        .child(crumbs)
        .into_any_element()
}
