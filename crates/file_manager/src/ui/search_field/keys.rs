//! Keyboard and clipboard commands of the [`SearchField`].
//!
//! Printable characters and IME output arrive through [`EntityInputHandler`]
//! (`input.rs`), so this module only handles the keys the platform does not turn
//! into text: caret movement, selection, delete/backspace, Escape/Enter and the
//! clipboard shortcuts. `gpui` dispatches key bindings before `on_key_down`, so
//! the panel forwards Ctrl+A/C/X/V here as well (see
//! `FileManagerPanel::focused_text_field`).

use gpui::{ClipboardItem, Context, EntityInputHandler, KeyDownEvent, Window};

use super::{SearchField, SearchFieldEvent};

impl SearchField {
    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
    }

    /// Copies the selection and deletes it.
    pub(crate) fn cut_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        self.copy_selection(cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    /// Inserts the clipboard text at the selection (newlines become spaces, so a
    /// pasted file path list cannot break the single-line field).
    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    /// Selects the whole text.
    pub(crate) fn select_all_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }
    fn backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let start = self.previous_boundary(self.cursor_offset());
            self.selected_range = start..self.cursor_offset();
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            self.selected_range = cursor..self.next_boundary(cursor);
        }
        self.replace_text_in_range(None, "", window, cx);
    }
    /// Inserts `text` at the caret (or over the selection).
    ///
    /// The panel uses this when it has the focus while the search mode is open,
    /// so typing keeps feeding the query instead of being dropped.
    pub(crate) fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, text, window, cx);
    }

    /// Deletes the selection, or one grapheme before the caret.
    pub(crate) fn delete_backward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.backspace(window, cx);
    }

    pub(super) fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        let extend = modifiers.shift;
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.emit(SearchFieldEvent::Dismiss);
                true
            }
            "enter" => {
                cx.emit(SearchFieldEvent::Submit);
                true
            }
            "backspace" => {
                self.backspace(window, cx);
                true
            }
            "delete" => {
                self.delete(window, cx);
                true
            }
            "left" => {
                if extend {
                    self.select_to(self.previous_boundary(self.cursor_offset()), window, cx);
                } else if self.selected_range.is_empty() {
                    self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
                } else {
                    self.move_to(self.selected_range.start, window, cx);
                }
                true
            }
            "right" => {
                if extend {
                    self.select_to(self.next_boundary(self.cursor_offset()), window, cx);
                } else if self.selected_range.is_empty() {
                    self.move_to(self.next_boundary(self.cursor_offset()), window, cx);
                } else {
                    self.move_to(self.selected_range.end, window, cx);
                }
                true
            }
            "home" => {
                if extend {
                    self.select_to(0, window, cx);
                } else {
                    self.move_to(0, window, cx);
                }
                true
            }
            "end" => {
                let end = self.content.len();
                if extend {
                    self.select_to(end, window, cx);
                } else {
                    self.move_to(end, window, cx);
                }
                true
            }
            "a" if modifiers.control => {
                self.select_all_text(window, cx);
                true
            }
            "c" if modifiers.control => {
                self.copy_selection(cx);
                true
            }
            "x" if modifiers.control => {
                self.cut_selection(window, cx);
                true
            }
            "v" if modifiers.control => {
                self.paste(window, cx);
                true
            }
            // Typed characters normally arrive through the platform's text
            // input (`replace_text_in_range`, which also carries IME output).
            // Inserting them here as well makes the field work even when the
            // platform has no input handler installed for the frame; stopping
            // the propagation keeps the platform from inserting them a second
            // time — gpui only forwards `key_char` when the event is still
            // unhandled (`gpui_linux` x11/wayland window.rs, `gpui_macos`).
            _ => {
                let Some(text) = super::typed_text(&event.keystroke) else {
                    return false;
                };
                self.replace_text_in_range(None, &text, window, cx);
                true
            }
        }
    }
}
