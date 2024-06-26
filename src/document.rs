use std::borrow::Cow;

use gpui::{prelude::FluentBuilder, *};
use helix_core::{
    ropey::RopeSlice,
    syntax::{Highlight, HighlightEvent},
};
use helix_lsp::lsp::{Diagnostic, DiagnosticSeverity, NumberOrString};
use helix_term::ui::EditorView;
use helix_view::{graphics::CursorKind, Document, DocumentId, Editor, Theme, View, ViewId};
use log::debug;

use crate::utils::color_to_hsla;
use crate::{Core, Input, InputEvent};

pub struct DocumentView {
    core: Model<Core>,
    input: Model<Input>,
    view_id: ViewId,
    style: TextStyle,
    focus: FocusHandle,
    is_focused: bool,
}

impl DocumentView {
    pub fn new(
        core: Model<Core>,
        input: Model<Input>,
        view_id: ViewId,
        style: TextStyle,
        focus: &FocusHandle,
        is_focused: bool,
    ) -> Self {
        Self {
            core,
            input,
            view_id,
            style,
            focus: focus.clone(),
            is_focused,
        }
    }

    pub fn set_focused(&mut self, is_focused: bool) {
        self.is_focused = is_focused;
    }

    fn get_diagnostics(&self, cx: &mut ViewContext<Self>) -> Vec<Diagnostic> {
        if !self.is_focused {
            return Vec::new();
        }

        let core = self.core.read(cx);
        let editor = &core.editor;

        let (cursor_pos, doc_id, first_row) = {
            let view = editor.tree.get(self.view_id);
            let doc_id = view.doc;
            let document = editor.document(doc_id).unwrap();
            let text = document.text();

            let primary_idx = document
                .selection(self.view_id)
                .primary()
                .cursor(text.slice(..));
            let cursor_pos = view.screen_coords_at_pos(document, text.slice(..), primary_idx);

            let anchor = view.offset.anchor;
            let first_row = text.char_to_line(anchor.min(text.len_chars()));
            (cursor_pos, doc_id, first_row)
        };
        let Some(cursor_pos) = cursor_pos else {
            return Vec::new();
        };

        let mut diags = Vec::new();
        if let Some(path) = editor.document(doc_id).and_then(|doc| doc.path()).cloned() {
            if let Some(diagnostics) = editor.diagnostics.get(&path) {
                for (diag, _) in diagnostics.iter().filter(|(diag, _)| {
                    let (start_line, end_line) =
                        (diag.range.start.line as usize, diag.range.end.line as usize);
                    let row = cursor_pos.row + first_row;
                    start_line <= row && row <= end_line
                }) {
                    diags.push(diag.clone());
                }
            }
        }
        diags
    }
}

impl EventEmitter<DismissEvent> for DocumentView {}

impl Render for DocumentView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        println!("{:?}: rendering document view", self.view_id);

        cx.on_focus_out(&self.focus, |this, cx| {
            let is_focused = this.focus.is_focused(cx);

            if this.is_focused != is_focused {
                this.is_focused = is_focused;
                cx.notify();
            }
            debug!(
                "{:?} document view focus changed OUT: {:?}",
                this.view_id, this.is_focused
            );
        })
        .detach();

        cx.on_focus_in(&self.focus, |this, cx| {
            let is_focused = this.focus.is_focused(cx);

            if this.is_focused != is_focused {
                this.is_focused = is_focused;
                cx.notify();
            }
            debug!(
                "{:?} document view focus changed IN: {:?}",
                this.view_id, this.is_focused
            );
        })
        .detach();

        let doc_id = {
            let editor = &self.core.read(cx).editor;
            let view = editor.tree.get(self.view_id);
            view.doc
        };

        let handle = ScrollHandle::default();
        let doc = DocumentElement::new(
            self.core.clone(),
            doc_id.clone(),
            self.view_id.clone(),
            self.style.clone(),
            &self.focus,
            self.is_focused,
        )
        .overflow_y_scroll()
        .track_scroll(&handle)
        .on_scroll_wheel(cx.listener(move |view, ev: &ScrollWheelEvent, cx| {
            use helix_core::movement::Direction;
            let view_id = view.view_id;
            let line_height = view.style.line_height_in_pixels(cx.rem_size());

            debug!("SCROLL WHEEL {:?}", ev);
            let delta = ev.delta.pixel_delta(line_height);
            if delta.y != px(0.) {
                let lines = delta.y / line_height;
                let direction = if lines > 0. {
                    Direction::Backward
                } else {
                    Direction::Forward
                };
                let line_count = 1 + lines.abs() as usize;

                view.input.update(cx, |_, cx| {
                    cx.emit(InputEvent::ScrollLines {
                        direction,
                        line_count,
                        view_id,
                    })
                });
            }
        }));

        let status = crate::statusline::StatusLine::new(
            self.core.clone(),
            doc_id.clone(),
            self.view_id,
            self.is_focused,
            self.style.clone(),
        );

        let diags = {
            let theme = self.core.read(cx).editor.theme.clone();

            self.get_diagnostics(cx).into_iter().map(move |diag| {
                cx.new_view(|_| DiagnosticView {
                    diagnostic: diag,
                    theme: theme.clone(),
                })
            })
        };

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(doc)
            .child(status)
            .child(
                div()
                    .flex()
                    .w(DefiniteLength::Fraction(0.33))
                    .h(DefiniteLength::Fraction(0.8))
                    .flex_col()
                    .absolute()
                    .top_8()
                    .right_5()
                    .gap_4()
                    .children(diags),
            )
    }
}

impl FocusableView for DocumentView {
    fn focus_handle(&self, _cx: &AppContext) -> FocusHandle {
        self.focus.clone()
    }
}

pub struct DocumentElement {
    core: Model<Core>,
    doc_id: DocumentId,
    view_id: ViewId,
    style: TextStyle,
    interactivity: Interactivity,
    focus: FocusHandle,
    is_focused: bool,
}

impl IntoElement for DocumentElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl DocumentElement {
    pub fn new(
        core: Model<Core>,
        doc_id: DocumentId,
        view_id: ViewId,
        style: TextStyle,
        focus: &FocusHandle,
        is_focused: bool,
    ) -> Self {
        Self {
            core,
            doc_id,
            view_id,
            style,
            interactivity: Interactivity::default(),
            focus: focus.clone(),
            is_focused,
        }
        .track_focus(&focus)
        .element
    }

    // These 3 methods are just proxies for EditorView
    // TODO: make a PR to helix to extract them from helix_term into helix_view or smth.
    fn doc_diagnostics_highlights<'d>(
        doc: &'d helix_view::Document,
        theme: &Theme,
    ) -> impl Iterator<Item = Vec<(usize, std::ops::Range<usize>)>> {
        EditorView::doc_diagnostics_highlights(doc, theme).into_iter()
    }

    fn doc_syntax_highlights<'d>(
        doc: &'d helix_view::Document,
        anchor: usize,
        height: u16,
        theme: &Theme,
    ) -> Box<dyn Iterator<Item = HighlightEvent> + 'd> {
        EditorView::doc_syntax_highlights(doc, anchor, height, theme)
    }

    fn doc_selection_highlights(
        mode: helix_view::document::Mode,
        doc: &Document,
        view: &View,
        theme: &Theme,
        cursor_shape_config: &helix_view::editor::CursorShapeConfig,
        is_window_focused: bool,
    ) -> Vec<(usize, std::ops::Range<usize>)> {
        EditorView::doc_selection_highlights(
            mode,
            doc,
            view,
            theme,
            cursor_shape_config,
            is_window_focused,
        )
    }

    fn overlay_highlights(
        mode: helix_view::document::Mode,
        doc: &Document,
        view: &View,
        theme: &Theme,
        cursor_shape_config: &helix_view::editor::CursorShapeConfig,
        is_window_focused: bool,
        is_view_focused: bool,
    ) -> impl Iterator<Item = HighlightEvent> {
        let mut overlay_highlights =
            EditorView::empty_highlight_iter(doc, view.offset.anchor, view.inner_area(doc).height);
        if is_view_focused {
            let highlights = helix_core::syntax::merge(
                overlay_highlights,
                Self::doc_selection_highlights(
                    mode,
                    doc,
                    view,
                    theme,
                    cursor_shape_config,
                    is_window_focused,
                ),
            );
            let focused_view_elements =
                EditorView::highlight_focused_view_elements(view, doc, theme);
            if focused_view_elements.is_empty() {
                overlay_highlights = Box::new(highlights)
            } else {
                overlay_highlights =
                    Box::new(helix_core::syntax::merge(highlights, focused_view_elements))
            }
        }

        for diagnostic in Self::doc_diagnostics_highlights(doc, theme) {
            // Most of the `diagnostic` Vecs are empty most of the time. Skipping
            // a merge for any empty Vec saves a significant amount of work.
            if diagnostic.is_empty() {
                continue;
            }
            overlay_highlights =
                Box::new(helix_core::syntax::merge(overlay_highlights, diagnostic));
        }

        overlay_highlights
    }

    fn highlight(
        editor: &Editor,
        doc: &Document,
        view: &View,
        theme: &Theme,
        is_view_focused: bool,
        anchor: usize,
        lines: u16,
        end_char: usize,
        fg_color: Hsla,
        font: Font,
    ) -> Vec<TextRun> {
        let mut runs = vec![];
        let overlay_highlights = Self::overlay_highlights(
            editor.mode(),
            doc,
            view,
            theme,
            &editor.config().cursor_shape,
            true,
            is_view_focused,
        );

        let syntax_highlights = Self::doc_syntax_highlights(doc, anchor, lines, theme);

        let mut syntax_styles = StyleIter {
            text_style: helix_view::graphics::Style::default(),
            active_highlights: Vec::with_capacity(64),
            highlight_iter: syntax_highlights,
            theme,
        };

        let mut overlay_styles = StyleIter {
            text_style: helix_view::graphics::Style::default(),
            active_highlights: Vec::with_capacity(64),
            highlight_iter: overlay_highlights,
            theme,
        };

        let mut syntax_span =
            syntax_styles
                .next()
                .unwrap_or((helix_view::graphics::Style::default(), 0, usize::MAX));
        let mut overlay_span = overlay_styles.next().unwrap_or((
            helix_view::graphics::Style::default(),
            0,
            usize::MAX,
        ));

        let mut position = anchor;
        loop {
            let (syn_style, syn_start, syn_end) = syntax_span;
            let (ovl_style, ovl_start, ovl_end) = overlay_span;

            /* if we are between highlights, insert default style */
            let (style, is_default) = if position < syn_start && position < ovl_start {
                (helix_view::graphics::Style::default(), true)
            } else {
                let mut style = helix_view::graphics::Style::default();
                if position >= syn_start && position < syn_end {
                    style = style.patch(syn_style);
                }
                if position >= ovl_start && position < ovl_end {
                    style = style.patch(ovl_style);
                }
                (style, false)
            };

            let fg = style
                .fg
                .and_then(|fg| color_to_hsla(fg))
                .unwrap_or(fg_color);
            let bg = style.bg.and_then(|bg| color_to_hsla(bg));
            let len = if is_default {
                std::cmp::min(syn_start, ovl_start) - position
            } else {
                std::cmp::min(
                    syn_end.checked_sub(position).unwrap_or(usize::MAX),
                    ovl_end.checked_sub(position).unwrap_or(usize::MAX),
                )
            };
            let underline = style.underline_color.and_then(color_to_hsla);
            let underline = underline.map(|color| UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: true,
            });

            let len = std::cmp::min(len, end_char);

            let run = TextRun {
                len,
                font: font.clone(),
                color: fg,
                background_color: bg,
                underline,
                strikethrough: None,
            };
            runs.push(run);
            position += len;

            if position >= end_char {
                break;
            }
            if position >= syn_end {
                syntax_span = syntax_styles.next().unwrap_or((style, 0, usize::MAX));
            }
            if position >= ovl_end {
                overlay_span = overlay_styles.next().unwrap_or((style, 0, usize::MAX));
            }
        }
        runs
    }
}

impl InteractiveElement for DocumentElement {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for DocumentElement {}

#[derive(Debug)]
#[allow(unused)]
pub struct DocumentLayout {
    rows: usize,
    columns: usize,
    line_height: Pixels,
    font_size: Pixels,
    cell_width: Pixels,
    hitbox: Option<Hitbox>,
}

struct RopeWrapper<'a>(RopeSlice<'a>);

impl<'a> Into<SharedString> for RopeWrapper<'a> {
    fn into(self) -> SharedString {
        let cow: Cow<'_, str> = self.0.into();
        cow.to_string().into() // this is crazy
    }
}

impl Element for DocumentElement {
    type RequestLayoutState = ();

    type PrepaintState = DocumentLayout;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        cx: &mut WindowContext,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        let layout_id = cx.request_layout(style, []);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        _before_layout: &mut Self::RequestLayoutState,
        cx: &mut WindowContext,
    ) -> Self::PrepaintState {
        debug!("editor bounds {:?}", bounds);
        let core = self.core.clone();
        self.interactivity
            .prepaint(id, bounds, bounds.size, cx, |_, _, hitbox, cx| {
                cx.with_content_mask(Some(ContentMask { bounds }), |cx| {
                    let font_id = cx.text_system().resolve_font(&self.style.font());
                    let font_size = self.style.font_size.to_pixels(cx.rem_size());
                    let line_height = self.style.line_height_in_pixels(cx.rem_size());
                    let em_width = cx
                        .text_system()
                        .typographic_bounds(font_id, font_size, 'm')
                        .unwrap()
                        .size
                        .width;
                    let cell_width = cx
                        .text_system()
                        .advance(font_id, font_size, 'm')
                        .unwrap()
                        .width;
                    let columns = (bounds.size.width / em_width).floor() as usize;
                    let rows = (bounds.size.height / line_height).floor() as usize;

                    core.update(cx, |core, _cx| {
                        let rect = helix_view::graphics::Rect {
                            x: 0,
                            y: 0,
                            width: columns as u16,
                            height: rows as u16,
                        };
                        let editor = &mut core.editor;
                        editor.resize(rect)
                    });
                    DocumentLayout {
                        hitbox,
                        rows,
                        columns,
                        line_height,
                        font_size,
                        cell_width,
                    }
                })
            })
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        after_layout: &mut Self::PrepaintState,
        cx: &mut WindowContext,
    ) {
        let focus = self.focus.clone();
        self.interactivity
            .on_mouse_down(MouseButton::Left, move |_ev, cx| {
                println!("MOUSE DOWN");
                cx.focus(&focus);
            });

        let is_focused = self.is_focused;

        self.interactivity
            .paint(id, bounds, after_layout.hitbox.as_ref(), cx, |_, cx| {
                let core = self.core.read(cx);
                let editor = &core.editor;

                let view = editor.tree.get(self.view_id);
                let _viewport = view.area;

                let theme = &editor.theme;
                let default_style = theme.get("ui.background");
                let bg_color = color_to_hsla(default_style.bg.unwrap()).unwrap_or(black());
                let cursor_style = theme.get("ui.cursor.primary");
                let bg = fill(bounds, bg_color);
                let fg_color = color_to_hsla(
                    default_style
                        .fg
                        .unwrap_or(helix_view::graphics::Color::White),
                )
                .unwrap_or(white());

                let document = editor.document(self.doc_id).unwrap();
                let text = document.text();

                let (_, cursor_kind) = editor.cursor();
                let primary_idx = document
                    .selection(self.view_id)
                    .primary()
                    .cursor(text.slice(..));
                let cursor_pos = view.screen_coords_at_pos(document, text.slice(..), primary_idx);

                let gutter_width = view.gutter_offset(document);
                let gutter_overflow = gutter_width == 0;
                if !gutter_overflow {
                    debug!("need to render gutter {}", gutter_width);
                }

                let cursor_text = None; // TODO

                let _cursor_row = cursor_pos.map(|p| p.row);
                let anchor = view.offset.anchor;
                let total_lines = text.len_lines();
                let first_row = text.char_to_line(anchor.min(text.len_chars()));
                // println!("first row is {}", row);
                let last_row = (first_row + after_layout.rows + 1).min(total_lines);
                // println!("first row is {first_row} last row is {last_row}");
                let end_char = text.line_to_char(std::cmp::min(last_row, total_lines));

                let text_view = text.slice(anchor..end_char);
                let str: SharedString = RopeWrapper(text_view).into();

                let runs = Self::highlight(
                    &editor,
                    document,
                    view,
                    theme,
                    self.is_focused,
                    anchor,
                    total_lines.min(after_layout.rows + 1) as u16,
                    end_char,
                    fg_color,
                    self.style.font(),
                );
                let shaped_lines = cx
                    .text_system()
                    .shape_text(str, after_layout.font_size, &runs, None)
                    .unwrap();

                cx.paint_quad(bg);

                let mut origin = bounds.origin;
                origin.x += px(2.) + (after_layout.cell_width * gutter_width as f32);
                origin.y += px(1.);

                // draw document
                for line in shaped_lines {
                    line.paint(origin, after_layout.line_height, cx).unwrap();
                    origin.y += after_layout.line_height;
                }
                // draw cursor
                if self.is_focused {
                    match (cursor_pos, cursor_kind) {
                        (Some(position), kind) => {
                            let helix_core::Position { row, col } = position;
                            let origin_y = after_layout.line_height * row as f32;
                            let origin_x =
                                after_layout.cell_width * ((col + gutter_width as usize) as f32);
                            let mut cursor_fg = cursor_style
                                .bg
                                .and_then(|fg| color_to_hsla(fg))
                                .unwrap_or(fg_color);
                            cursor_fg.a = 0.5;

                            let mut cursor = Cursor {
                                origin: gpui::Point::new(origin_x, origin_y),
                                kind,
                                color: cursor_fg,
                                block_width: after_layout.cell_width,
                                line_height: after_layout.line_height,
                                text: cursor_text,
                            };
                            let mut origin = bounds.origin;
                            origin.x += px(2.);
                            origin.y += px(1.);

                            cursor.paint(origin, cx);
                        }
                        (None, _) => {}
                    }
                }
                // draw gutter
                {
                    let mut gutter_origin = bounds.origin;
                    gutter_origin.x += px(2.);
                    gutter_origin.y += px(1.);

                    let core = self.core.read(cx);
                    let editor = &core.editor;
                    let theme = &editor.theme;
                    let view = editor.tree.get(self.view_id);
                    let document = editor.document(self.doc_id).unwrap();
                    let lines = (first_row..last_row)
                        .enumerate()
                        .map(|(visual_line, doc_line)| LinePos {
                            first_visual_line: true,
                            doc_line,
                            visual_line: visual_line as u16,
                            start_char_idx: 0,
                        });

                    let mut gutter = Gutter {
                        after_layout,
                        text_system: cx.text_system().clone(),
                        lines: Vec::new(),
                        style: self.style.clone(),
                        origin: gutter_origin,
                    };
                    {
                        let mut gutters = Vec::new();
                        Gutter::init_gutter(
                            &editor,
                            document,
                            view,
                            theme,
                            is_focused,
                            &mut gutters,
                        );
                        for line in lines {
                            for gut in &mut gutters {
                                gut(line, &mut gutter)
                            }
                        }
                    }
                    for (origin, line) in gutter.lines {
                        line.paint(origin, after_layout.line_height, cx).unwrap();
                    }
                }
            });
    }
}

struct Gutter<'a> {
    after_layout: &'a DocumentLayout,
    text_system: std::sync::Arc<WindowTextSystem>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    style: TextStyle,
    origin: Point<Pixels>,
}

impl<'a> Gutter<'a> {
    fn init_gutter<'d>(
        editor: &'d Editor,
        doc: &'d Document,
        view: &'d View,
        theme: &Theme,
        is_focused: bool,
        gutters: &mut Vec<GutterDecoration<'d, Self>>,
    ) {
        let text = doc.text().slice(..);
        let cursors: std::rc::Rc<[_]> = doc
            .selection(view.id)
            .iter()
            .map(|range| range.cursor_line(text))
            .collect();

        let mut offset = 0;

        let gutter_style = theme.get("ui.gutter");
        let gutter_selected_style = theme.get("ui.gutter.selected");
        let gutter_style_virtual = theme.get("ui.gutter.virtual");
        let gutter_selected_style_virtual = theme.get("ui.gutter.selected.virtual");

        for gutter_type in view.gutters() {
            let mut gutter = gutter_type.style(editor, doc, view, theme, is_focused);
            let width = gutter_type.width(view, doc);
            // avoid lots of small allocations by reusing a text buffer for each line
            let mut text = String::with_capacity(width);
            let cursors = cursors.clone();
            let gutter_decoration = move |pos: LinePos, renderer: &mut Self| {
                // TODO handle softwrap in gutters
                let selected = cursors.contains(&pos.doc_line);
                let x = offset;
                let y = pos.visual_line;

                let gutter_style = match (selected, pos.first_visual_line) {
                    (false, true) => gutter_style,
                    (true, true) => gutter_selected_style,
                    (false, false) => gutter_style_virtual,
                    (true, false) => gutter_selected_style_virtual,
                };

                if let Some(style) =
                    gutter(pos.doc_line, selected, pos.first_visual_line, &mut text)
                {
                    renderer.render(x, y, width, gutter_style.patch(style), Some(&text));
                } else {
                    renderer.render(x, y, width, gutter_style, None);
                }
                text.clear();
            };
            gutters.push(Box::new(gutter_decoration));

            offset += width as u16;
        }
    }
}

impl<'a> GutterRenderer for Gutter<'a> {
    fn render(
        &mut self,
        x: u16,
        y: u16,
        _width: usize,
        style: helix_view::graphics::Style,
        text: Option<&str>,
    ) {
        let origin_y = self.origin.y + self.after_layout.line_height * y as f32;
        let origin_x = self.origin.x + self.after_layout.cell_width * x as f32;

        let fg_color = style
            .fg
            .and_then(color_to_hsla)
            .unwrap_or(hsla(0., 0., 1., 1.));
        if let Some(text) = text {
            let run = TextRun {
                len: text.len(),
                font: self.style.font(),
                color: fg_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = self
                .text_system
                .shape_line(text.to_string().into(), self.after_layout.font_size, &[run])
                .unwrap();
            self.lines.push((
                Point {
                    x: origin_x,
                    y: origin_y,
                },
                shaped,
            ));
        }
    }
}

struct Cursor {
    origin: gpui::Point<Pixels>,
    kind: CursorKind,
    color: Hsla,
    block_width: Pixels,
    line_height: Pixels,
    text: Option<ShapedLine>,
}

impl Cursor {
    fn bounds(&self, origin: gpui::Point<Pixels>) -> Bounds<Pixels> {
        match self.kind {
            CursorKind::Bar => Bounds {
                origin: self.origin + origin,
                size: size(px(2.0), self.line_height),
            },
            CursorKind::Block => Bounds {
                origin: self.origin + origin,
                size: size(self.block_width, self.line_height),
            },
            CursorKind::Underline => Bounds {
                origin: self.origin
                    + origin
                    + gpui::Point::new(Pixels::ZERO, self.line_height - px(2.0)),
                size: size(self.block_width, px(2.0)),
            },
            CursorKind::Hidden => todo!(),
        }
    }

    pub fn paint(&mut self, origin: gpui::Point<Pixels>, cx: &mut WindowContext) {
        let bounds = self.bounds(origin);

        let cursor = fill(bounds, self.color);

        cx.paint_quad(cursor);

        if let Some(text) = &self.text {
            text.paint(self.origin + origin, self.line_height, cx)
                .unwrap();
        }
    }
}

type GutterDecoration<'a, T> = Box<dyn FnMut(LinePos, &mut T) + 'a>;

trait GutterRenderer {
    fn render(
        &mut self,
        x: u16,
        y: u16,
        width: usize,
        style: helix_view::graphics::Style,
        text: Option<&str>,
    );
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct LinePos {
    /// Indicates whether the given visual line
    /// is the first visual line of the given document line
    pub first_visual_line: bool,
    /// The line index of the document line that contains the given visual line
    pub doc_line: usize,
    /// Vertical offset from the top of the inner view area
    pub visual_line: u16,
    /// The first char index of this visual line.
    /// Note that if the visual line is entirely filled by
    /// a very long inline virtual text then this index will point
    /// at the next (non-virtual) char after this visual line
    pub start_char_idx: usize,
}

// TODO: copy-pasted from helix_term ui/document.rs

/// A wrapper around a HighlightIterator
/// that merges the layered highlights to create the final text style
/// and yields the active text style and the char_idx where the active
/// style will have to be recomputed.
struct StyleIter<'a, H: Iterator<Item = HighlightEvent>> {
    text_style: helix_view::graphics::Style,
    active_highlights: Vec<Highlight>,
    highlight_iter: H,
    theme: &'a Theme,
}

impl<H: Iterator<Item = HighlightEvent>> Iterator for StyleIter<'_, H> {
    type Item = (helix_view::graphics::Style, usize, usize);

    fn next(&mut self) -> Option<(helix_view::graphics::Style, usize, usize)> {
        while let Some(event) = self.highlight_iter.next() {
            match event {
                HighlightEvent::HighlightStart(highlights) => {
                    self.active_highlights.push(highlights)
                }
                HighlightEvent::HighlightEnd => {
                    self.active_highlights.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if start == end {
                        continue;
                    }
                    let style = self
                        .active_highlights
                        .iter()
                        .fold(self.text_style, |acc, span| {
                            acc.patch(self.theme.highlight(span.0))
                        });
                    return Some((style, start, end));
                }
            }
        }
        None
    }
}

struct DiagnosticView {
    diagnostic: Diagnostic,
    theme: Theme,
}

impl Render for DiagnosticView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        debug!("rendering diag {:?}", self.diagnostic);

        fn color(style: helix_view::graphics::Style) -> Hsla {
            style.fg.and_then(color_to_hsla).unwrap_or(white())
        }

        let theme = &self.theme;
        let text_style = theme.get("ui.text.info");
        let popup_style = theme.get("ui.popup.info");
        let warning = theme.get("warning");
        let error = theme.get("error");
        let info = theme.get("info");
        let hint = theme.get("hint");

        let fg = text_style.fg.and_then(color_to_hsla).unwrap_or(white());
        let bg = popup_style.bg.and_then(color_to_hsla).unwrap_or(black());

        let title_color = match self.diagnostic.severity {
            Some(DiagnosticSeverity::WARNING) => color(warning),
            Some(DiagnosticSeverity::ERROR) => color(error),
            Some(DiagnosticSeverity::INFORMATION) => color(info),
            Some(DiagnosticSeverity::HINT) => color(hint),
            _ => fg,
        };

        let font = cx.global::<crate::FontSettings>().fixed_font.clone();
        let source_and_code = self.diagnostic.source.as_ref().and_then(|src| {
            let code = self.diagnostic.code.as_ref();
            let code_str = code.map(|code| match code {
                NumberOrString::Number(num) => num.to_string(),
                NumberOrString::String(str) => str.to_string(),
            });
            Some(format!("{}: {}", src, code_str.unwrap_or_default()))
        });

        div()
            .p_2()
            .gap_2()
            .shadow_sm()
            .rounded_sm()
            .bg(black())
            .flex()
            .flex_col()
            .font(font)
            .text_size(px(12.))
            .text_color(fg)
            .bg(bg)
            .child(
                div()
                    .flex()
                    .font_weight(FontWeight::BOLD)
                    .text_color(title_color)
                    .justify_center()
                    .items_center()
                    .when_some(source_and_code, |this, source| this.child(source.clone())),
            )
            .child(div().flex_col().child(self.diagnostic.message.clone()))
    }
}
