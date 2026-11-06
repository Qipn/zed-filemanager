//! Panel geometry: the sidebar width, the details pane toggle and the
//! responsive breakpoints.
//!
//! OpenXplorer adapts at 1190 / 960 / 680 px of *window* width; inside a Zed
//! dock the panel can be far narrower than the window, so the same thresholds
//! are applied to the panel's own measured width.

use gpui::Context;

use super::FileManagerPanel;
use crate::ui::ox_theme::metrics;

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

    /// Sets the width of the sidebar, clamped to OpenXplorer's range.
    pub(crate) fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.sidebar_width = width.clamp(140., 560.);
        cx.notify();
    }

    /// Ends a sidebar drag. Kept for callers that want to cancel one; the
    /// drag itself is owned by GPUI (see `SidebarResizeDrag`), so nothing has
    /// to be cleaned up here.
    pub(crate) fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        self.sidebar_width = metrics::SIDEBAR_WIDTH;
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

    /// The effective sidebar width: the user's width, but never more than a
    /// third of a narrow panel.
    pub(crate) fn effective_sidebar_width(&self) -> f32 {
        let width = self.breakpoint_width();
        if width < 700. {
            self.sidebar_width.min((width * 0.42).max(140.))
        } else {
            self.sidebar_width
        }
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
