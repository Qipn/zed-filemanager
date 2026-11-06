//! Quick look: `Space` previews the selected entry in the details pane.
//!
//! GNOME Files previews on `Space` through `nautilus-previewer.c`; here the
//! preview is the details pane, which already shows the selected entry: pressing
//! space opens it (`Space` again closes it) and, for text files, loads the
//! beginning of the file so the pane can show the content instead of a generic
//! icon. Images are drawn by the pane itself.

use std::path::{Path, PathBuf};

use gpui::{Context, SharedString, Window};
use util::ResultExt;

use super::FileManagerPanel;

/// How much of a text file the preview reads.
const PREVIEW_MAX_BYTES: u64 = 64 * 1024;

/// Extensions the preview reads as text.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "rs", "c", "h", "cpp", "hpp", "py", "js", "ts", "tsx", "jsx", "go", "java", "rb",
    "sh", "toml", "json", "yaml", "yml", "nix", "lua", "zig", "csv", "log", "conf", "ini", "html",
    "css", "xml",
];

/// Whether the preview should draw `path` as an image.
pub(crate) fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| {
            matches!(
                extension.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avif" | "svg"
            )
        })
}

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension.as_str()))
}

impl FileManagerPanel {
    /// `Space`: open the details pane with a preview of the selection, or close
    /// it again when the same entry is already being previewed.
    pub(crate) fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            return;
        };

        // A second space on the entry that is already previewed closes the pane,
        // the way Quick Look toggles.
        if self.details_pane_visible
            && self.preview_path.as_ref() == Some(&entry.path)
            && !entry.is_dir
        {
            self.details_pane_visible = false;
            self.preview_path = None;
            self.preview_text = None;
            self.persist_state(cx);
            cx.notify();
            return;
        }

        self.details_pane_visible = true;
        self.persist_state(cx);
        self.load_preview(entry.path, window, cx);
        cx.notify();
    }

    /// Loads the text preview of `path`, if it is a text file small enough to
    /// read.
    fn load_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_path = Some(path.clone());
        self.preview_text = None;
        if !is_text(&path) {
            return;
        }

        let fs = self.fs.clone();
        self._operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let size = fs.metadata(&path).await.ok().flatten().map(|meta| meta.len);
            if size.is_some_and(|size| size > PREVIEW_MAX_BYTES) {
                return;
            }
            let Ok(text) = fs.load(&path).await else {
                return;
            };
            let snippet: SharedString = text.chars().take(2000).collect::<String>().into();
            this.update(cx, |this, cx| {
                // A newer preview may have replaced this one meanwhile.
                if this.preview_path.as_ref() == Some(&path) {
                    this.preview_text = Some(snippet);
                    cx.notify();
                }
            })
            .log_err();
        }));
    }

    /// The text snippet of the previewed entry, when it belongs to `path`.
    pub(crate) fn preview_text_for(&self, path: &Path) -> Option<SharedString> {
        if self.preview_path.as_deref() == Some(path) {
            self.preview_text.clone()
        } else {
            None
        }
    }
}
