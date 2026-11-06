//! A single-line text field for the search mode of the location slot.
//!
//! GNOME Files' search box is a real `Gtk.Text` inside
//! `NautilusQueryEditor` (`nautilus-query-editor.blp`): a prefix icon, the
//! entry, a clear button and the filter button. The entry itself supports
//! everything a text widget does — caret movement, selection, clipboard and
//! input methods — so Chinese/Japanese/Korean input works.
//!
//! The panel cannot use Zed's `Editor` (the `editor` crate depends on this
//! crate for "reveal in file manager", so depending back would form a cycle),
//! and `ui` has no text field. This module therefore implements the gpui way:
//! a small entity that implements [`EntityInputHandler`] and paints its text
//! with an [`ElementInputHandler`] registered during paint
//! (`crates/gpui/examples/input.rs` is the upstream example).
//!
//! Only the entry is modelled here; the prefix icon and the clear button are
//! drawn by the panel, which owns the surrounding state.
//!
//! The same entity is reused as the inline rename editor (the panel keeps a
//! second instance): the details and grid views render it in place of the name
//! while `rename_state` points at the row.
//!
//! The module is split by concern: this file holds the state and the gpui
//! entity plumbing, `keys.rs` the keyboard/clipboard commands,
//! `input.rs` the [`EntityInputHandler`] bridge and `element.rs` the painting.

mod element;
mod input;
mod keys;

use element::SearchFieldElement;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine,
    SharedString, Window, px,
};
use ui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

/// What the panel has to react to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchFieldEvent {
    /// The text changed; the panel restarts the search.
    Change,
    /// <kbd>Enter</kbd> was pressed: the panel hands the focus back to the
    /// list so the selected hit can be opened.
    Submit,
    /// <kbd>Escape</kbd> was pressed: the panel leaves the search mode.
    Dismiss,
}

/// The single-line entry of the search mode.
pub(crate) struct SearchField {
    /// The element id of the painted area; the search entry and the inline
    /// rename editor must not share it (`ElementId` collisions break hit
    /// testing).
    element_id: SharedString,
    focus_handle: FocusHandle,
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// The shaped line of the last frame, used to map x → index and to paint.
    last_layout: Option<ShapedLine>,
    /// The bounds of the text area in the last frame.
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    placeholder: SharedString,
}

impl EventEmitter<SearchFieldEvent> for SearchField {}

impl SearchField {
    pub(crate) fn new(element_id: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            element_id: element_id.into(),
            focus_handle: cx.focus_handle(),
            content: String::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            placeholder: "Search".into(),
        }
    }

    /// The current text.
    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    /// Replaces the text without notifying the panel (used when the search
    /// opens or closes).
    pub(crate) fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.content.clear();
        self.content.push_str(text);
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    /// Replaces the text and selects `selection` (byte offsets), like
    /// `project_panel` selecting only the file stem when a rename starts
    /// (`rename_impl`: "Only files keep their extension unselected for quick
    /// renames").
    pub(crate) fn set_text_with_selection(
        &mut self,
        text: &str,
        selection: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_text(text, window, cx);
        let start = selection.start.min(self.content.len());
        let end = selection.end.min(self.content.len()).max(start);
        self.selected_range = start..end;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// Clears the text and notifies the panel.
    pub(crate) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_text("", window, cx);
        cx.emit(SearchFieldEvent::Change);
    }

    /// Copies the selection to the clipboard.

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return self.content.len();
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }
}

impl Focusable for SearchField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The painted text of the field: the line, the selection and the caret.

impl Render for SearchField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(self.element_id.clone())
            .flex_1()
            .min_w(px(0.))
            .h(px(20.))
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.focus_handle)
            .text_size(px(13.))
            .line_height(px(20.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.handle_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    // Clicking the entry must not hand the focus back to the
                    // panel (its root focuses the panel on every click).
                    cx.stop_propagation();
                    window.focus(&this.focus_handle, cx);
                    let index = this.index_for_mouse_position(event.position);
                    this.move_to(index, window, cx);
                    this.is_selecting = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.is_selecting = false;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.is_selecting = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.is_selecting {
                    let index = this.index_for_mouse_position(event.position);
                    this.select_to(index, window, cx);
                }
            }))
            .child(SearchFieldElement { field: cx.entity() })
    }
}
