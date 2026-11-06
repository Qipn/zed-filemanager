//! The details view's columns: their widths and their visibility.
//!
//! `Name` always takes the remaining width (it is the only flexible column);
//! `Date modified`, `Type` and `Size` have a width the user can drag, the way
//! OpenXplorer's column resizers work (`ui-spec.md` §4.5). The widths and the
//! visibility survive a restart through [`super::persistence`].

use gpui::{Bounds, Context, Pixels};

use super::FileManagerPanel;
use crate::ui::ox_theme::metrics;

/// A column the user can resize and hide. `Name` is not one of them: it is the
/// flexible column and hiding it would leave no names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column {
    Modified,
    Type,
    Size,
}

impl Column {
    /// Every resizable column, left to right.
    pub(crate) const ALL: [Column; 3] = [Column::Modified, Column::Type, Column::Size];

    /// The index into the panel's `column_widths` / `columns_shown` arrays.
    pub(crate) fn index(self) -> usize {
        match self {
            Column::Modified => 0,
            Column::Type => 1,
            Column::Size => 2,
        }
    }

    /// The column for an array index, for drag payloads.
    pub(crate) fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub(crate) fn default_width(self) -> f32 {
        match self {
            Column::Modified => metrics::COLUMN_MODIFIED,
            Column::Type => metrics::COLUMN_TYPE,
            Column::Size => metrics::COLUMN_SIZE,
        }
    }

    /// The label used by the view menu's visibility toggles.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Column::Modified => "Date modified",
            Column::Type => "Type",
            Column::Size => "Size",
        }
    }
}

/// Narrowest a column may be dragged to; keeps the header text readable.
pub(crate) const MIN_COLUMN_WIDTH: f32 = 70.;
/// Widest a column may be dragged to.
pub(crate) const MAX_COLUMN_WIDTH: f32 = 480.;

impl FileManagerPanel {
    /// The width of a resizable column in logical pixels.
    pub(crate) fn column_width(&self, column: Column) -> f32 {
        self.column_widths[column.index()]
    }

    /// Whether a column is shown: the user's toggle, plus the responsive rule
    /// that drops the columns a narrow dock cannot fit.
    pub(crate) fn is_column_shown(&self, column: Column) -> bool {
        if !self.columns_shown[column.index()] {
            return false;
        }
        match column {
            // OpenXplorer drops `Date modified` at 640 and `Type` at 760.
            Column::Modified => self.breakpoint_width() >= 640.,
            Column::Type => self.breakpoint_width() >= 760.,
            Column::Size => true,
        }
    }

    /// Shows or hides a column, like Nautilus' column chooser.
    pub(crate) fn toggle_column(&mut self, column: Column, cx: &mut Context<Self>) {
        let index = column.index();
        self.columns_shown[index] = !self.columns_shown[index];
        self.persist_state(cx);
        cx.notify();
    }

    /// Sets a column's width, clamped to what is still readable and useful.
    pub(crate) fn set_column_width(&mut self, column: Column, width: f32, cx: &mut Context<Self>) {
        let width = width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        let index = column.index();
        if (self.column_widths[index] - width).abs() < 0.5 {
            return;
        }
        self.column_widths[index] = width;
        self.persist_state(cx);
        cx.notify();
    }

    /// Resizes `column` for a drag at `x` on the header occupying `header`.
    ///
    /// The column's left edge is reconstructed from the header's width: `Name`
    /// takes whatever is left after the fixed columns and the right padding, and
    /// it never goes below its minimum, so the same calculation holds when the
    /// dock is too narrow for the sum of the defaults.
    pub(crate) fn resize_column_at(
        &mut self,
        column: Column,
        x: f32,
        header: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let fixed: f32 = Column::ALL
            .iter()
            .filter(|column| self.is_column_shown(**column))
            .map(|column| self.column_width(*column))
            .sum();
        let name_width = (f32::from(header.size.width) - fixed - metrics::COLUMN_EDGE_PADDING)
            .max(metrics::COLUMN_NAME_MIN);

        let mut left = f32::from(header.left()) + name_width;
        for candidate in Column::ALL {
            if candidate == column {
                break;
            }
            if self.is_column_shown(candidate) {
                left += self.column_width(candidate);
            }
        }

        self.set_column_width(column, x - left, cx);
    }

    /// Restores the default column widths.
    pub(crate) fn reset_column_widths(&mut self, cx: &mut Context<Self>) {
        self.column_widths = Column::ALL.map(Column::default_width);
        self.persist_state(cx);
        cx.notify();
    }
}
