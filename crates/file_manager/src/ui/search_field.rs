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

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    fill, point, px, relative, size,
};
use theme::ActiveTheme;
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

    /// Handles the keys the platform does not translate into text input.
    ///
    /// Returns `true` when the key was consumed. Printable characters and IME
    /// compositions arrive through [`EntityInputHandler`] instead, so they are
    /// deliberately not handled here.
    fn handle_key(
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
                if modifiers.control || modifiers.alt || modifiers.platform {
                    return false;
                }
                let Some(text) = event.keystroke.key_char.as_deref() else {
                    return false;
                };
                if text.is_empty() {
                    return false;
                }
                self.replace_text_in_range(None, text, window, cx);
                true
            }
        }
    }
}

impl Focusable for SearchField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for SearchField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range.take();
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(SearchFieldEvent::Change);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content.replace_range(range.clone(), new_text);
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());

        let insertion_start_utf16 = self.offset_to_utf16(range.start);
        let inserted_length_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(insertion_start_utf16 + selected.start.min(inserted_length_utf16)
                        ..insertion_start_utf16 + selected.end.min(inserted_length_utf16)),
                )
            })
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        self.selection_reversed = false;

        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(SearchFieldEvent::Change);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.content {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.content {
            return None;
        }
        let utf8_index = last_layout.closest_index_for_x(line_point.x);
        Some(self.offset_to_utf16(utf8_index))
    }
}

/// The painted text of the field: the line, the selection and the caret.
struct SearchFieldElement {
    field: gpui::Entity<SearchField>,
}

impl IntoElement for SearchFieldElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct SearchFieldPrepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl Element for SearchFieldElement {
    type RequestLayoutState = ();
    type PrepaintState = SearchFieldPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let content = field.content.clone();
        let selected_range = field.selected_range.clone();
        let cursor = field.cursor_offset();
        let style = window.text_style();
        let colors = cx.theme().colors();

        let (display_text, text_color) = if content.is_empty() {
            (field.placeholder.clone(), colors.text_placeholder)
        } else {
            (content.clone().into(), colors.text)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if content.is_empty() {
            vec![run]
        } else if let Some(marked_range) = field.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        // An empty field shows the placeholder, which has no selection or
        // caret to place.
        let (selection, cursor) = if content.is_empty() {
            (None, None)
        } else if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + line.x_for_index(cursor), bounds.top()),
                        size(px(1.), bounds.bottom() - bounds.top()),
                    ),
                    colors.text_accent,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    colors.element_selection_background,
                )),
                None,
            )
        };

        SearchFieldPrepaint {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.field.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        line.paint(
            bounds.origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            cx,
        )
        .ok();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.field.update(cx, |field, _cx| {
            field.last_layout = Some(line);
            field.last_bounds = Some(bounds);
        });
    }
}

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
