//! Drag payloads and the floating ghost, shared by the panel, the folder view
//! and the sidebar resizer.

use std::path::PathBuf;

use gpui::{Context, IntoElement, Render, SharedString, Window};
use theme::ActiveTheme;
use ui::prelude::*;

/// The panel's internal drag payload: paths being dragged between entries.
#[derive(Clone, Debug)]
pub(crate) struct DraggedPaths(pub(crate) Vec<PathBuf>);

/// The drag payload of the sidebar resizer.
///
/// Zed's dock does the same thing with `DraggedDock`
/// (`crates/workspace/src/dock.rs`): the handle only starts a drag, GPUI owns
/// the drag lifecycle, and the container resizes on every `on_drag_move`.
/// Releasing the mouse therefore always ends the drag.
#[derive(Clone)]
pub(crate) struct SidebarResizeDrag;

impl Render for SidebarResizeDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Small floating view shown while dragging entries.
pub(crate) struct DragGhost {
    pub(crate) labels: Vec<SharedString>,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .px_2()
            .py_1()
            .rounded_xs()
            .bg(cx.theme().colors().element_selected)
            .child(
                Label::new(self.labels.first().cloned().unwrap_or_default())
                    .single_line()
                    .size(LabelSize::Small),
            )
            .when(self.labels.len() > 1, |label| {
                label.child(
                    Label::new(format!("+{}", self.labels.len() - 1))
                        .single_line()
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
    }
}
