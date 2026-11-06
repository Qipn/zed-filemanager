//! The header bar: the sidebar toggle, history buttons, the shared location
//! slot (breadcrumbs, editable path, or search) and the view/new controls.
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

use gpui::{AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, Window, div, px};
use theme::ActiveTheme;
use ui::{Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, Tooltip, prelude::*};

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

/// The shared location slot of the header bar.
///
/// GNOME Files (Nautilus) uses a single field for three modes instead of an
/// address box plus a separate search box: the breadcrumb path, the editable
/// location, and the search query. The panel does the same: the slot shows the
/// search while `search_query` is set, the editable path while `path_edit` is
/// set, and the breadcrumbs otherwise.
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

    if this.path_edit.is_some() {
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
    let skipped = segments.len().saturating_sub(max_segments);
    let visible = &segments[skipped..];

    // The crumbs must not shrink: a squeezed label is both unreadable and hard
    // to hit.
    let mut crumbs = h_flex()
        .gap(px(2.))
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden();

    if skipped > 0 {
        crumbs = crumbs.child(
            Label::new("…")
                .size(LabelSize::Small)
                .color(Color::Custom(palette.text_tertiary)),
        );
    }

    for (offset, (target, name)) in visible.iter().enumerate() {
        let index = skipped + offset;
        let is_last = offset + 1 == visible.len();
        if offset > 0 || skipped > 0 {
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
                    .flex_shrink_0()
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
                            // A crumb opens its jump menu; the address bar's own
                            // handler must not also enter the edit mode.
                            cx.stop_propagation();
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
        .min_w(px(120.))
        .px(px(8.))
        .py(px(2.))
        .items_center()
        .rounded_xs()
        .bg(palette.field_bg)
        .border_1()
        .border_color(palette.border)
        // Clicking the bar itself (the icon, the gaps between crumbs, the space
        // after the last one) enters the editable path.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.begin_path_edit(window, cx);
            }),
        )
        .child(
            Icon::new(IconName::Folder)
                .size(IconSize::Small)
                .color(Color::Custom(palette.muted)),
        )
        .child(crumbs)
        .child(
            // The chevron re-enters the edit mode. It stops the mouse-down so the
            // panel root's background handler does not cancel the edit first.
            div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    IconButton::new("file-manager-edit-path", IconName::ChevronDown)
                        .icon_size(IconSize::XSmall)
                        .tooltip(Tooltip::text("Edit location (Ctrl+L)"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.begin_path_edit(window, cx)),
                        ),
                ),
        )
        .into_any_element()
}
