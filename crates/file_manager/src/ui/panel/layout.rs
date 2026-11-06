//! Panel geometry: the sidebar width, the details pane toggle and the
//! responsive breakpoints.
//!
//! OpenXplorer adapts at 1190 / 960 / 680 px of *window* width; inside a Zed
//! dock the panel can be far narrower than the window, so the same thresholds
//! are applied to the panel's own measured width.

use gpui::Context;

use super::FileManagerPanel;
use crate::ui::ox_theme::metrics;

/// Narrowest the sidebar may be dragged to.
const MIN_SIDEBAR_WIDTH: f32 = 120.;
/// Room the folder view always keeps, so dragging never hides it.
const MIN_CONTENT_WIDTH: f32 = 200.;
/// Smallest share of the sidebar the places list may take.
const MIN_SIDEBAR_SPLIT: f32 = 0.12;
/// Largest share of the sidebar the places list may take.
const MAX_SIDEBAR_SPLIT: f32 = 0.88;

impl FileManagerPanel {
    /// Shows or hides the details pane.
    pub(crate) fn toggle_details_pane(&mut self, cx: &mut Context<Self>) {
        self.details_pane_visible = !self.details_pane_visible;
        cx.notify();
    }

    /// Shows or hides the sidebar.
    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Sets the width of the sidebar.
    ///
    /// The drag is free between a narrow minimum and a generous maximum; the
    /// visible cap comes from [`Self::effective_sidebar_width`], which keeps a
    /// minimum amount of room for the folder view.
    pub(crate) fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = width.clamp(120., 720.);
        if (self.sidebar_width - width).abs() < 0.5 {
            return;
        }
        self.sidebar_width = width;
        cx.notify();
    }

    /// Ends a sidebar drag. Kept for callers that want to cancel one; the
    /// drag itself is owned by GPUI (see `SidebarResizeDrag`), so nothing has
    /// to be cleaned up here.
    pub(crate) fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        self.sidebar_width = metrics::SIDEBAR_WIDTH;
        cx.notify();
    }

    /// Sets how much of the sidebar height the places list takes (0…1).
    ///
    /// Called on every drag move of the sidebar's internal splitter, so it is
    /// clamped to keep both panes usable.
    pub(crate) fn set_sidebar_split(&mut self, ratio: f32, cx: &mut Context<Self>) {
        let ratio = ratio.clamp(MIN_SIDEBAR_SPLIT, MAX_SIDEBAR_SPLIT);
        if (self.sidebar_split - ratio).abs() < 0.002 {
            return;
        }
        self.sidebar_split = ratio;
        cx.notify();
    }

    /// Restores the even split between places and the folder tree.
    pub(crate) fn reset_sidebar_split(&mut self, cx: &mut Context<Self>) {
        self.sidebar_split = 0.5;
        cx.notify();
    }

    // ---- Responsive layout ----

    /// Records the width the panel was laid out at.
    ///
    /// OpenXplorer adapts its layout to the window width at 1190, 960 and
    /// 680 px (`native/docs/ui-spec.md` §4). Inside a Zed dock the panel can be
    /// much narrower than the window, so the same thresholds are applied to the
    /// panel's own width instead.
    pub(crate) fn set_content_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = width.round();
        if self.content_width == Some(width) {
            return;
        }
        self.content_width = Some(width);
        cx.notify();
    }

    /// The width used for the breakpoints, defaulting to a wide panel.
    fn breakpoint_width(&self) -> f32 {
        self.content_width.unwrap_or(1200.)
    }

    /// Whether the sidebar has room right now. Below 520 px the folder view
    /// needs the space more than the navigation pane does.
    pub(crate) fn is_sidebar_shown(&self) -> bool {
        self.sidebar_visible && self.breakpoint_width() >= 520.
    }

    /// Whether the details pane fits. OpenXplorer hides it at ≤ 960.
    pub(crate) fn is_details_pane_shown(&self) -> bool {
        self.details_pane_visible && self.breakpoint_width() >= 900.
    }

    /// The width the sidebar actually gets.
    ///
    /// The user's width wins as long as the folder view keeps at least
    /// [`MIN_CONTENT_WIDTH`] px; only then is the sidebar shortened. This keeps
    /// the drag feeling free instead of snapping to a fixed fraction of a
    /// narrow dock.
    pub(crate) fn effective_sidebar_width(&self) -> f32 {
        let panel = self.breakpoint_width();
        let max = (panel - MIN_CONTENT_WIDTH).max(MIN_SIDEBAR_WIDTH);
        self.sidebar_width.min(max).max(MIN_SIDEBAR_WIDTH)
    }

    /// Whether the `Date modified` column fits.
    pub(crate) fn is_modified_column_shown(&self) -> bool {
        self.breakpoint_width() >= 640.
    }

    /// Whether the `Type` column fits (OpenXplorer drops it at ≤ 680).
    pub(crate) fn is_type_column_shown(&self) -> bool {
        self.breakpoint_width() >= 760.
    }
}
