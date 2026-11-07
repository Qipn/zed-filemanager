//! The status bar: item counts, the selection, the icon size stepper and the
//! view toggles.

use gpui::{AnyElement, Context, IntoElement, div, px};
use theme::ActiveTheme;
use ui::{
    ButtonStyle, Color, IconButton, IconName, IconSize, Label, LabelSize, TintColor, Tooltip,
    prelude::*,
};

use crate::model::clipboard::Clipboard;
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::views::ViewMode;
/// The status bar: the item count, the selection and the view toggles.
pub(super) fn render_status_bar(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    // While searching the counts describe the hits, not the folder
    // (`nautilus-files-view.c` shows the same status text in both modes).
    let searching = this.is_searching();
    // A broad query stops at `MAX_RESULTS`; say so instead of lying about the
    // total.
    let truncated = searching
        && this
            .search_results
            .as_ref()
            .is_some_and(|results| results.truncated);
    let shown: &[crate::model::entries::DirEntry] = if searching {
        this.search_results
            .as_ref()
            .map(|results| results.entries.as_slice())
            .unwrap_or(&[])
    } else {
        &this.entries
    };

    let count = if truncated {
        format!("{}+", shown.len())
    } else {
        shown.len().to_string()
    };
    let mut status = if searching {
        if this.search_running {
            format!("Searching… {count}")
        } else {
            format!("{count} results")
        }
    } else {
        format!("{} items", this.entries.len())
    };
    // OpenXplorer shows the total size next to the item count.
    let total: u64 = shown.iter().filter_map(|entry| entry.len).sum();
    if total > 0 {
        status.push_str(&format!("  {}", crate::model::entries::format_len(total)));
    }
    match this.selected.len() {
        0 => {}
        1 => {
            if let Some(entry) = this.selected_entry() {
                let size = entry
                    .len
                    .map(crate::model::entries::format_len)
                    .unwrap_or_else(|| "—".into());
                status.push_str(&format!(" — {} ({})", entry.name, size));
            }
        }
        count => status.push_str(&format!(" — {count} selected")),
    }
    if this.using_fallback_dir {
        status.push_str(" — no workspace folder");
    }
    let clipboard_hint = match &this.clipboard {
        Some(Clipboard::Copied(_)) => " (copied)",
        Some(Clipboard::Cut(_)) => " (cut)",
        Some(Clipboard::Linked(_)) => " (linked)",
        None => "",
    };

    h_flex()
        .id("file-manager-status-bar")
        .h(px(metrics::STATUS_BAR))
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(18.))
        .px(px(13.))
        .bg(palette.bg)
        .border_t_1()
        .border_color(palette.divider)
        .child(
            Label::new(format!("{status}{clipboard_hint}"))
                .single_line()
                .size(LabelSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        // The active sort order. GNOME Files shows it in its view menu; a dock
        // has the room for it in the status bar.
        .child(
            Label::new(format!("Sorted by {}", this.sort_mode.label()))
                .single_line()
                .size(LabelSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .child(div().flex_1())
        // How full the drive behind the current folder is (`statvfs`).
        .when_some(this.disk_space, |bar, (total, available)| {
            bar.child(render_disk_space(total, available, cx))
        })
        // The icon size stepper, only in the icon grid view.
        .when(this.view_mode == ViewMode::Grid, |this| {
            this.child(
                IconButton::new("file-manager-icon-smaller", IconName::SquareMinus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Smaller icons"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.decrease_icon_size(cx);
                    })),
            )
            .child(
                IconButton::new("file-manager-icon-larger", IconName::Plus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Larger icons"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.increase_icon_size(cx);
                    })),
            )
        })
        // The view toggles, at the right of the status bar. The active view
        // is highlighted like OpenXplorer's status mode (§4.7).
        .child(
            IconButton::new("file-manager-view-details", IconName::Table)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Details view"))
                .toggle_state(this.view_mode == ViewMode::Details)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.view_mode != ViewMode::Details {
                        this.set_view_mode(ViewMode::Details, cx);
                    }
                })),
        )
        .child(
            IconButton::new("file-manager-view-icons", IconName::FolderOpen)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Large icons"))
                .toggle_state(this.view_mode == ViewMode::Grid)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.view_mode != ViewMode::Grid {
                        this.set_view_mode(ViewMode::Grid, cx);
                    }
                })),
        )
        .into_any_element()
}

/// The drive capacity: a small bar plus the free space, like OpenXplorer's
/// status-bar capacity indicator.
fn render_disk_space(total: u64, available: u64, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());
    let used_ratio = if total == 0 {
        0.
    } else {
        ((total.saturating_sub(available)) as f32 / total as f32).clamp(0., 1.)
    };
    // Running out of room is worth a warning colour.
    let full = available.saturating_mul(10) < total;
    let fill = if full { palette.danger } else { palette.accent };

    h_flex()
        .id("file-manager-disk-space")
        .gap(px(6.))
        .flex_shrink_0()
        .child(
            div()
                .w(px(56.))
                .h(px(6.))
                .rounded_full()
                .bg(palette.divider)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(px(56. * used_ratio))
                        .rounded_full()
                        .bg(fill),
                ),
        )
        .child(
            Label::new(format!(
                "{} free of {}",
                crate::model::entries::format_len(available),
                crate::model::entries::format_len(total)
            ))
            .single_line()
            .size(LabelSize::XSmall)
            .color(Color::Custom(palette.muted)),
        )
        .into_any_element()
}
