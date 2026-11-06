//! OpenXplorer design tokens, ported 1:1 from the upstream UI specification
//! (`native/docs/ui-spec.md` in the OpenXplorer repository).
//!
//! The tokens are split in two groups:
//!
//! - **Metrics**: fixed pixel sizes that define the layout skeleton (band
//!   heights, column widths, icon sizes). They are constants so the layout can
//!   be checked against the spec at a glance.
//! - **Colours**: resolved from the active Zed theme so the panel follows the
//!   editor's light/dark appearance while keeping OpenXplorer's palette
//!   character (soft blue selection, muted secondary text, hairline borders).

use gpui::Hsla;
use theme::Theme;

/// Layout metrics, in logical pixels at 100 % text size.
///
/// Source: `native/docs/ui-spec.md` §1.1 ("Band heights", "Widths", "Sizes").
/// Some values describe parts that are not rendered yet (the details pane, the
/// search field); they are kept here so the layout stays comparable with the
/// specification.
#[allow(dead_code)]
pub(crate) mod metrics {
    /// Height of the header bar (sidebar toggle, history, location slot,
    /// search, view and new controls).
    ///
    /// GNOME Files' `Adw.HeaderBar` is 47 px tall (34 px controls + 6 px
    /// padding); the toolbar of this panel follows that single-row layout
    /// (`nautilus-toolbar.blp`) instead of OpenXplorer's 62 px navigation row
    /// plus 55 px command bar.
    pub(crate) const HEADER_BAR: f32 = 47.;
    /// Height of the column header row of the details view.
    pub(crate) const COLUMN_HEADER: f32 = 38.;
    /// Height of the status bar.
    pub(crate) const STATUS_BAR: f32 = 30.;
    /// Vertical pitch of one row in the details view.
    pub(crate) const ROW_PITCH: f32 = 38.;

    /// Default width of the sidebar (places + folder tree).
    pub(crate) const SIDEBAR_WIDTH: f32 = 210.;
    /// Width of the resizer between the sidebar and the content.
    pub(crate) const SIDEBAR_RESIZER: f32 = 6.;
    /// Width of the details pane on the right.
    pub(crate) const DETAILS_PANE_WIDTH: f32 = 262.;
    /// Width of the search field.
    pub(crate) const SEARCH_WIDTH: f32 = 235.;

    /// Icon size of a row in the details view.
    pub(crate) const ROW_ICON: f32 = 21.;
    /// Icon size of the tiles in the icon grid view.
    pub(crate) const TILE_ICON: f32 = 56.;
    /// Size of a tile in the icon grid view.
    pub(crate) const TILE_WIDTH: f32 = 135.;
    /// Size of a tile in the icon grid view.
    pub(crate) const TILE_HEIGHT: f32 = 130.;
    /// The icon sizes of the grid view, in logical pixels.
    ///
    /// OpenXplorer offers fourteen steps (16 … 256); the default is "Large"
    /// (56 px).
    pub(crate) const ICON_SIZES: [f32; 14] = [
        16., 22., 28., 32., 40., 48., 56., 64., 80., 96., 128., 160., 192., 256.,
    ];
    /// The index of the default icon size ("Large", 56 px).
    pub(crate) const DEFAULT_ICON_SIZE: usize = 6;
    /// Height of one row in the sidebar.
    pub(crate) const SIDEBAR_ROW: f32 = 35.;
    /// Height of one row in the folder tree.
    pub(crate) const TREE_ROW: f32 = 30.;
    /// Indentation per tree level.
    pub(crate) const TREE_INDENT: f32 = 16.;

    /// Column widths of the details view.
    pub(crate) const COLUMN_NAME_MIN: f32 = 235.;
    pub(crate) const COLUMN_MODIFIED: f32 = 176.;
    pub(crate) const COLUMN_TYPE: f32 = 135.;
    pub(crate) const COLUMN_SIZE: f32 = 90.;
    /// Trailing padding that GTK adds to the first and the last column.
    pub(crate) const COLUMN_EDGE_PADDING: f32 = 14.;
}

/// Palette, resolved from the Zed theme.
///
/// The colours follow OpenXplorer's tokens (§3.1 of the UI spec) as closely as
/// Zed's theme allows: `chrome` and `bg` come from the panel surfaces,
/// `selected` keeps OpenXplorer's soft blue, `muted` is the secondary text
/// colour.
pub(crate) struct OxPalette {
    /// Chrome surfaces: the header bar.
    pub(crate) chrome: Hsla,
    /// Content surface: the folder view.
    pub(crate) bg: Hsla,
    /// Sidebar surface.
    pub(crate) sidebar: Hsla,
    /// Hairline borders between bands.
    ///
    /// Zed uses `border` for control strokes (high contrast) and
    /// `border_variant` for "a visual divider between two sections"
    /// (`crates/theme/src/styles/colors.rs`). The bands of the file manager
    /// are dividers, so they use `border_variant`.
    pub(crate) border: Hsla,
    /// Deemphasized divider colour, matching Zed's `ui::Divider` default.
    pub(crate) divider: Hsla,
    /// Background of a drop target, matching Zed's drag highlight
    /// (`colors.drop_target_background`, used by project_panel / pane).
    pub(crate) drop_target: Hsla,
    /// Primary text.
    pub(crate) text: Hsla,
    /// Secondary text (column values, status bar).
    pub(crate) muted: Hsla,
    /// Tertiary text (breadcrumb separators).
    pub(crate) text_tertiary: Hsla,
    /// Accent colour (selection edges, focus rings, caret).
    pub(crate) accent: Hsla,
    /// Row hover background.
    pub(crate) hover: Hsla,
    /// Selected row background (OpenXplorer's soft blue).
    pub(crate) selected: Hsla,
    /// Field background (address bar).
    pub(crate) field_bg: Hsla,
}

impl OxPalette {
    /// Builds the palette from the active theme.
    pub(crate) fn new(theme: &Theme) -> Self {
        let colors = theme.colors();
        Self {
            chrome: colors.panel_background,
            bg: colors.element_background,
            sidebar: colors.panel_background,
            border: colors.border,
            divider: colors.border_variant,
            drop_target: colors.drop_target_background,
            text: colors.text,
            muted: colors.text_muted,
            text_tertiary: colors.text_muted,
            accent: colors.text_accent,
            hover: colors.element_hover,
            // OpenXplorer selects rows with a soft blue tint (C19).
            selected: colors.element_selected,
            field_bg: colors.element_background,
        }
    }
}

/// Convenience accessor for the palette of the current theme.
pub(crate) fn palette(theme: &Theme) -> OxPalette {
    OxPalette::new(theme)
}
