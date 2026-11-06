//! View state: the details/grid mode, the icon size and the folders expanded
//! in place by the details view.

use std::path::Path;

use gpui::{Context, Window};
use util::ResultExt;

use super::FileManagerPanel;
use crate::model::entries::read_entries;
use crate::ui::ox_theme::metrics;
use crate::ui::views::ViewMode;

impl FileManagerPanel {
    /// Sets the view mode.
    pub(crate) fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        cx.notify();
    }

    /// Expands or collapses a directory in the folder view.
    ///
    /// The children are read the first time the directory is opened, like
    /// OpenXplorer's expandable folders.
    pub(crate) fn toggle_entry_expanded(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.expanded_dirs.remove(path).is_some() {
            cx.notify();
            return;
        }

        let fs = self.fs.clone();
        let show_hidden = self.show_hidden;
        let sort_mode = self.sort_mode;
        let path = path.to_path_buf();
        self._loading_task = Some(cx.spawn_in(window, async move |this, cx| {
            let children = read_entries(fs, path.clone(), show_hidden, sort_mode).await;
            this.update(cx, |this, cx| {
                match children {
                    Ok(children) => {
                        this.expanded_dirs.insert(path.clone(), children);
                        this.error = None;
                    }
                    Err(error) => {
                        this.error = Some(format!("{error}").into());
                    }
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    /// Whether a directory is expanded in the folder view.
    pub(crate) fn is_entry_expanded(&self, path: &Path) -> bool {
        self.expanded_dirs.contains_key(path)
    }

    /// The icon size of the grid view, in logical pixels.
    pub(crate) fn icon_size(&self) -> f32 {
        metrics::ICON_SIZES[self.icon_size.min(metrics::ICON_SIZES.len() - 1)]
    }

    /// Makes the icons of the grid view one step larger.
    pub(crate) fn increase_icon_size(&mut self, cx: &mut Context<Self>) {
        if self.icon_size + 1 < metrics::ICON_SIZES.len() {
            self.icon_size += 1;
            cx.notify();
        }
    }

    /// Makes the icons of the grid view one step smaller.
    pub(crate) fn decrease_icon_size(&mut self, cx: &mut Context<Self>) {
        if self.icon_size > 0 {
            self.icon_size -= 1;
            cx.notify();
        }
    }
}
