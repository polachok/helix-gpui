use std::borrow::Cow;

use gpui::*;
use helix_core::{
    graphemes::ensure_grapheme_boundary_next_byte,
    ropey::RopeSlice,
    syntax::{Highlight, HighlightEvent},
};
use helix_term::keymap::Keymaps;
use helix_view::{graphics::CursorKind, Document, DocumentId, Editor, Theme, View, ViewId};

use crate::utils::{color_to_hsla, translate_key};

pub struct DocumentView {
    editor: Model<Editor>,
    keymaps: Model<Keymaps>,
    doc_id: DocumentId,
    view_id: ViewId,
    style: TextStyle,
    interactivity: Interactivity,
    focus: FocusHandle,
    is_focused: bool,
    rt_handle: tokio::runtime::Handle,
}

impl DocumentView {
    pub fn new(
        editor: Model<Editor>,
        keymaps: Model<Keymaps>,
        doc_id: DocumentId,
        view_id: ViewId,
        style: TextStyle,
        focus: &FocusHandle,
        is_focused: bool,
        rt_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            editor,
            keymaps,
            doc_id,
            view_id,
            style,
            interactivity: Interactivity::default(),
            focus: focus.clone(),
            is_focused,
            rt_handle,
        }
        .track_focus(&focus)
        .element
    }

    fn install_key_handlers(
        editor: Model<Editor>,
        keymaps: Model<Keymaps>,
        rt_handle: tokio::runtime::Handle,
        view_id: ViewId,
        cx: &mut ElementContext,
    ) {
        let mode = {
            let editor = editor.read(cx);
            editor.mode()
        };

        // self.interactivity
        // .on_mouse_down(MouseButton::Left, move |ev, cx| {
        //     println!("MOUSE DOWN");
        //     cx.focus(&focus);
        //     // TODO: change focus in editor
        // });

        cx.on_key_event::<KeyDownEvent>(move |ev, phase, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            println!("KEY EVENT {:?} @ {:?}", ev, phase);
            let key = translate_key(&ev.keystroke);

            println!("KEY EVENT {:?}", key);
            let res = keymaps.update(cx, |keymaps, _cx| keymaps.get(mode, key));
            println!("res {:?}", res);
            let res = editor.update(cx, |editor, cx| {
                let _guard = rt_handle.enter();
                let mut ctx = helix_term::commands::Context {
                    editor,
                    register: None,
                    count: None,
                    callback: Vec::new(),
                    on_next_key_callback: None,
                    jobs: &mut helix_term::job::Jobs::new(),
                };
                let res = handle_key_result(mode, &mut ctx, res);
                editor.ensure_cursor_in_view(view_id);
                cx.notify();
                drop(_guard);
                cx.emit(crate::Update);
                res
            });
            println!("res {:?}", res);
        });
    }

    fn viewport_byte_range(
        text: helix_core::RopeSlice,
        row: usize,
        height: u16,
    ) -> std::ops::Range<usize> {
        // Calculate viewport byte ranges:
        // Saturating subs to make it inclusive zero indexing.
        let last_line = text.len_lines().saturating_sub(1);
        let last_visible_line = (row + height as usize).saturating_sub(1).min(last_line);
        let start = text.line_to_byte(row.min(last_line));
        let end = text.line_to_byte(last_visible_line + 1);

        start..end
    }

    fn syntax_highlights(doc: &helix_view::Document, anchor: usize, height: u16) -> Highlights {
        let text = doc.text().slice(..);
        let row = text.char_to_line(anchor.min(text.len_chars()));
        let range = Self::viewport_byte_range(text, row, height);

        match doc.syntax() {
            Some(syn) => {
                let iter = syn
                    // TODO: range doesn't actually restrict source, just highlight range
                    .highlight_iter(text.slice(..), Some(range), None)
                    .map(|event| event.unwrap())
                    .map(move |event| match event {
                        // TODO: use byte slices directly
                        // convert byte offsets to char offset
                        HighlightEvent::Source { start, end } => {
                            let start =
                                text.byte_to_char(ensure_grapheme_boundary_next_byte(text, start));
                            let end =
                                text.byte_to_char(ensure_grapheme_boundary_next_byte(text, end));
                            HighlightEvent::Source { start, end }
                        }
                        event => event,
                    });
                let mut regions = vec![];
                let mut current_region = HighlightRegion {
                    start: 0,
                    end: 0,
                    hl: Highlight(0),
                };
                for event in iter {
                    match event {
                        HighlightEvent::HighlightStart(highlight) => {
                            current_region.hl = highlight;
                        }
                        HighlightEvent::Source { start, end } => {
                            current_region.start = start;
                            current_region.end = end;
                        }
                        HighlightEvent::HighlightEnd => {
                            regions.push(current_region);
                        }
                    }
                }
                Highlights(regions)
            }
            None => Highlights(vec![HighlightRegion {
                start: text.byte_to_char(range.start),
                end: text.byte_to_char(range.end),
                hl: Highlight(0),
            }]),
        }
    }

    fn init_gutter<'d, T>(
        editor: &'d Editor,
        doc: &'d Document,
        view: &'d View,
        theme: &Theme,
        is_focused: bool,
        gutters: &mut Vec<GutterDecoration<'d, T>>,
    ) where
        T: GutterRenderer,
    {
        let text = doc.text().slice(..);
        let viewport = view.area;
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
            let gutter_decoration = move |pos: LinePos, renderer: &mut T| {
                // TODO handle softwrap in gutters
                let selected = cursors.contains(&pos.doc_line);
                let x = viewport.x + offset;
                let y = viewport.y + pos.visual_line;

                let gutter_style = match (selected, pos.first_visual_line) {
                    (false, true) => gutter_style,
                    (true, true) => gutter_selected_style,
                    (false, false) => gutter_style_virtual,
                    (true, false) => gutter_selected_style_virtual,
                };

                if let Some(style) =
                    gutter(pos.doc_line, selected, pos.first_visual_line, &mut text)
                {
                    // println!(
                    //     "{:?} {:?} `{text}``",
                    //     gutter_type,
                    //     gutter_style.patch(style)
                    // );
                    renderer.render(x, y, width, gutter_style.patch(style), Some(&text));
                } else {
                    renderer.render(x, y, width, gutter_style, None);
                    // println!("{:?} {:?} `{text}` `{width}`", gutter_type, gutter_style);
                }
                // {
                //     renderer
                //         .surface
                //         .set_stringn(x, y, &text, width, gutter_style.patch(style));
                // } else {
                //     renderer.surface.set_style(
                //         Rect {
                //             x,
                //             y,
                //             width: width as u16,
                //             height: 1,
                //         },
                //         gutter_style,
                //     );
                // }
                text.clear();
            };
            gutters.push(Box::new(gutter_decoration));

            offset += width as u16;
        }
    }
}

impl InteractiveElement for DocumentView {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for DocumentView {}

#[derive(Debug)]
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

impl Element for DocumentView {
    type BeforeLayout = ();

    type AfterLayout = DocumentLayout;

    fn before_layout(&mut self, cx: &mut ElementContext) -> (LayoutId, Self::BeforeLayout) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        let layout_id = cx.with_element_context(|cx| cx.request_layout(&style, None));
        (layout_id, ())
    }

    fn after_layout(
        &mut self,
        bounds: Bounds<Pixels>,
        _before_layout: &mut Self::BeforeLayout,
        cx: &mut ElementContext,
    ) -> Self::AfterLayout {
        println!("EDITOR BOUNDS {:?}", bounds);
        let editor = self.editor.clone();
        // cx.observe_keystrokes(move |ev, cx| {
        //     use helix_view::input::{Event, KeyCode, KeyEvent, KeyModifiers};
        //     println!("{:?}", ev);
        //     let chars = ev.keystroke.key.chars().collect::<Vec<char>>();
        //     if chars.len() == 1 {
        //         let code = KeyCode::Char(chars[0]);
        //         let kev = KeyEvent {
        //             code,
        //             modifiers: KeyModifiers::NONE,
        //         };
        //         let ev = Event::Key(kev);
        //         editor.update(cx, |editor, cx| {
        //             // TODO:
        //         });
        //     }
        // })
        // .detach();
        self.interactivity
            .after_layout(bounds, bounds.size, cx, |_, _, hitbox, cx| {
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

                    editor.update(cx, |editor, _cx| {
                        let rect = helix_view::graphics::Rect {
                            x: 0,
                            y: 0,
                            width: columns as u16,
                            height: rows as u16,
                        };
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
        bounds: Bounds<Pixels>,
        _: &mut Self::BeforeLayout,
        after_layout: &mut Self::AfterLayout,
        cx: &mut ElementContext,
    ) {
        // println!("{:?} {:?} {:?}", self.doc_id, after_layout, bounds);
        let focus = self.focus.clone();
        self.interactivity
            .on_mouse_down(MouseButton::Left, move |_ev, cx| {
                println!("MOUSE DOWN");
                cx.focus(&focus);
            });

        let lh = after_layout.line_height;
        let editor = self.editor.clone();
        let view_id = self.view_id;
        self.interactivity.on_scroll_wheel(move |ev, cx| {
            use helix_core::movement::Direction;
            // println!("SCROLL WHEEL {:?}", ev);
            let delta = ev.delta.pixel_delta(lh);
            if delta.y != px(0.) {
                let lines = delta.y / lh;
                let direction = if lines > 0. {
                    Direction::Backward
                } else {
                    Direction::Forward
                };
                let line_count = 1 + lines.abs() as usize;

                // println!("{:?}", line_count);
                editor.update(cx, |editor, cx| {
                    let mut ctx = helix_term::commands::Context {
                        editor,
                        register: None,
                        count: None,
                        callback: Vec::new(),
                        on_next_key_callback: None,
                        jobs: &mut helix_term::job::Jobs::new(),
                    };
                    helix_term::commands::scroll(&mut ctx, line_count, direction, false);

                    editor.ensure_cursor_in_view(view_id);
                    cx.notify();
                    cx.emit(crate::Update);
                });
            }
        });

        let is_focused = self.is_focused;
        self.interactivity
            .paint(bounds, after_layout.hitbox.as_ref(), cx, |_, cx| {
                if is_focused {
                    cx.focus(&self.focus);
                }
                let keymaps = self.keymaps.clone();
                let editor = self.editor.clone();

                let view_id = self.view_id;
                let rt_handle = self.rt_handle.clone();

                Self::install_key_handlers(editor, keymaps, rt_handle, view_id, cx);

                let editor = self.editor.read(cx);

                let view = editor.tree.get(self.view_id);
                // println!("offset {:?}", view.offset);
                let cursor = editor.cursor();
                let (cursor_pos, _cursor_kind) = cursor;
                // println!("cursor @ {:?}", cursor);

                let theme = &editor.theme;
                let default_style = theme.get("ui.background");
                let bg_color = color_to_hsla(default_style.bg.unwrap());
                let window_style = theme.get("ui.window");
                let border_color = color_to_hsla(window_style.fg.unwrap());
                let cursor_style = theme.get("ui.cursor.primary");
                let bg = fill(bounds, bg_color);
                let borders = outline(bounds, border_color);
                let fg_color = color_to_hsla(
                    default_style
                        .fg
                        .unwrap_or(helix_view::graphics::Color::White),
                );

                let document = editor.document(self.doc_id).unwrap();

                let gutter_width = view.gutter_offset(document);
                let gutter_overflow = gutter_width == 0;
                if !gutter_overflow {
                    println!("need to render gutter {}", gutter_width);
                }

                let text = document.text();

                let cursor_text = None; // TODO

                let cursor_row = cursor_pos.map(|p| p.row);
                // println!("ROW: {:?}", cursor_row);
                let anchor = view.offset.anchor;
                let total_lines = text.len_lines();
                let first_row = text.char_to_line(anchor.min(text.len_chars()));
                // println!("first row is {}", row);
                let last_row = (first_row + after_layout.rows + 1).min(total_lines);
                // println!("last row is {}", last_row);
                let end_char = text.line_to_char(std::cmp::min(last_row, total_lines));

                let text_view = text.slice(anchor..end_char);
                let str: SharedString = RopeWrapper(text_view).into();

                // TODO: refactor all highlighting into separate function
                let highlights = Self::syntax_highlights(
                    document,
                    anchor,
                    total_lines.min(after_layout.rows + 1) as u16,
                );
                let regions = highlights.get(anchor, end_char);

                let mut runs = vec![];
                let mut previous_end = anchor;
                let mut previous_region: Option<HighlightRegion> = None;

                for reg in regions {
                    let HighlightRegion { start, end, hl } = *reg;

                    if let Some(prev) = previous_region {
                        if prev.start == start && prev.end == end {
                            println!(
                                "replacing previous region {:?} with new region {:?}",
                                reg, prev
                            );
                            runs.pop();
                        }
                    }

                    if start > end_char || previous_end > end_char {
                        break;
                    }

                    // if previous run didn't end at the start of this region
                    // we have to insert default run
                    if start > previous_end {
                        let len = start - previous_end;

                        let run = TextRun {
                            len,
                            font: self.style.font(),
                            color: fg_color,
                            background_color: Some(bg_color),
                            underline: None,
                            strikethrough: None,
                        };
                        runs.push(run);
                    }

                    let style = theme.highlight(hl.0);
                    let fg = style.fg.map(|fg| color_to_hsla(fg));
                    //let bg = style.fg.map(|fg| color_to_hsla(fg));
                    let len = end - start;
                    let run = TextRun {
                        len,
                        font: self.style.font(),
                        color: fg.unwrap_or(fg_color),
                        background_color: Some(bg_color),
                        underline: None,
                        strikethrough: None,
                    };
                    runs.push(run);
                    previous_end = end;
                    previous_region = Some(*reg);
                }

                let shaped_lines = cx
                    .text_system()
                    .shape_text(str, after_layout.font_size, &runs, None)
                    .unwrap();

                cx.paint_quad(bg);
                //cx.paint_quad(borders);

                let mut origin = bounds.origin;
                origin.x += px(2.) + (after_layout.cell_width * gutter_width as f32);
                origin.y += px(1.);
                // draw document
                for line in shaped_lines {
                    line.paint(origin, after_layout.line_height, cx).unwrap();
                    origin.y += after_layout.line_height;
                }
                // draw cursor
                match cursor {
                    (Some(position), kind) => {
                        let helix_core::Position { row, col } = position;
                        let origin_y = after_layout.line_height * row as f32;
                        let origin_x = after_layout.cell_width * col as f32;
                        let mut cursor_fg = cursor_style
                            .bg
                            .map(|fg| color_to_hsla(fg))
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
                // draw gutter
                {
                    let editor = self.editor.read(cx);
                    let theme = &editor.theme;
                    let view = editor.tree.get(self.view_id);
                    let document = editor.document(self.doc_id).unwrap();
                    let it = (first_row..last_row)
                        .enumerate()
                        .map(|(visual_line, doc_line)| LinePos {
                            first_visual_line: true,
                            doc_line,
                            visual_line: visual_line as u16,
                            start_char_idx: 0,
                        });

                    struct Lol<'a> {
                        after_layout: &'a DocumentLayout,
                        text_system: std::sync::Arc<WindowTextSystem>,
                        lines: Vec<(Point<Pixels>, ShapedLine)>,
                        style: TextStyle,
                        origin: Point<Pixels>,
                    }
                    impl<'a> GutterRenderer for Lol<'a> {
                        fn render(
                            &mut self,
                            x: u16,
                            y: u16,
                            _width: usize,
                            style: helix_view::graphics::Style,
                            text: Option<&str>,
                        ) {
                            // println!("RENDERING GUTTER {:?} with width {width} @ {x} {y}", text);
                            let origin_y = self.origin.y + self.after_layout.line_height * y as f32;
                            let origin_x = self.origin.x + self.after_layout.cell_width * x as f32;
                            let fg_color =
                                style.fg.map(color_to_hsla).unwrap_or(hsla(0., 0., 1., 1.));
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
                                    .shape_line(
                                        text.to_string().into(),
                                        self.after_layout.font_size,
                                        &[run],
                                    )
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

                    let mut lol = Lol {
                        after_layout,
                        text_system: cx.text_system().clone(),
                        lines: Vec::new(),
                        style: self.style.clone(),
                        origin: bounds.origin,
                    };
                    {
                        let mut gutters = Vec::new();
                        Self::init_gutter::<Lol>(
                            editor,
                            document,
                            view,
                            theme,
                            is_focused,
                            &mut gutters,
                        );
                        for line in it {
                            for gutter in &mut gutters {
                                gutter(line, &mut lol)
                            }
                        }
                    }
                    for (origin, line) in lol.lines {
                        line.paint(origin, after_layout.line_height, cx).unwrap();
                    }
                }
            });
    }
}

/// Handle events by looking them up in `self.keymaps`. Returns None
/// if event was handled (a command was executed or a subkeymap was
/// activated). Only KeymapResult::{NotFound, Cancelled} is returned
/// otherwise.
fn handle_key_result(
    mode: helix_view::document::Mode,
    cxt: &mut helix_term::commands::Context,
    key_result: helix_term::keymap::KeymapResult,
) -> Option<helix_term::keymap::KeymapResult> {
    use helix_term::events::{OnModeSwitch, PostCommand};
    use helix_term::keymap::KeymapResult;
    use helix_view::document::Mode;

    let mut last_mode = mode;
    //self.pseudo_pending.extend(self.keymaps.pending());
    //let key_result = keymaps.get(mode, event);
    //cxt.editor.autoinfo = keymaps.sticky().map(|node| node.infobox());

    let mut execute_command = |command: &helix_term::commands::MappableCommand| {
        command.execute(cxt);
        helix_event::dispatch(PostCommand { command, cx: cxt });

        let current_mode = cxt.editor.mode();
        if current_mode != last_mode {
            helix_event::dispatch(OnModeSwitch {
                old_mode: last_mode,
                new_mode: current_mode,
                cx: cxt,
            });

            // HAXX: if we just entered insert mode from normal, clear key buf
            // and record the command that got us into this mode.
            if current_mode == Mode::Insert {
                // how we entered insert mode is important, and we should track that so
                // we can repeat the side effect.
                //self.last_insert.0 = command.clone();
                //self.last_insert.1.clear();
            }
        }

        last_mode = current_mode;
    };

    match &key_result {
        KeymapResult::Matched(command) => {
            execute_command(command);
        }
        KeymapResult::Pending(node) => cxt.editor.autoinfo = Some(node.infobox()),
        KeymapResult::MatchedSequence(commands) => {
            for command in commands {
                execute_command(command);
            }
        }
        KeymapResult::NotFound | KeymapResult::Cancelled(_) => return Some(key_result),
    }
    None
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

    pub fn paint(&mut self, origin: gpui::Point<Pixels>, cx: &mut ElementContext) {
        let bounds = self.bounds(origin);

        let cursor = fill(bounds, self.color);

        cx.paint_quad(cursor);

        if let Some(text) = &self.text {
            text.paint(self.origin + origin, self.line_height, cx)
                .unwrap();
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct HighlightRegion {
    start: usize,
    end: usize,
    hl: Highlight,
}

#[derive(Debug)]
struct Highlights(Vec<HighlightRegion>);

impl Highlights {
    fn get(&self, start: usize, end: usize) -> Vec<&HighlightRegion> {
        let mut highlights = vec![];
        for region in &self.0 {
            if region.start >= end || region.end <= start {
                continue;
            }
            highlights.push(region);
        }
        highlights
    }
}

impl IntoElement for DocumentView {
    type Element = Self;

    fn into_element(self) -> Self {
        self
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
