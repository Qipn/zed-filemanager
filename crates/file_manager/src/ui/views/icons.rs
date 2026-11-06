//! Entry icons, resolved through Zed's `file_icons` crate.
//!
//! `FileIcons::get_icon` looks a path up in the active icon theme: exact file
//! names first (`Dockerfile`, `eslint.config.js`), then suffixes/stems, and it
//! covers directories too (`directory_icons` / `named_directory_icons`). One
//! call therefore serves files and folders, and the panel inherits every icon
//! theme Zed ships instead of drawing two generic glyphs.
//!
//! When the theme has no icon for the entry we fall back to the generic
//! `IconName` glyphs with the panel's own accent/muted colours.

use file_icons::FileIcons;
use gpui::{AnyElement, App};
use ui::prelude::*;
use ui::{Color, Icon, IconName, IconSize};

use crate::model::entries::DirEntry;

/// The icon of `entry` at the given size.
pub(crate) fn entry_icon(entry: &DirEntry, size: IconSize, cx: &App) -> AnyElement {
    match FileIcons::get_icon(&entry.path, cx) {
        // Theme icons carry their own colours (and the folder variant differs
        // between directories), so they are drawn without a tint.
        Some(glyph) => Icon::from_path(glyph).size(size).into_any_element(),
        None => Icon::new(if entry.is_dir {
            IconName::Folder
        } else {
            IconName::File
        })
        .size(size)
        .color(if entry.is_dir {
            Color::Accent
        } else {
            Color::Muted
        })
        .into_any_element(),
    }
}
