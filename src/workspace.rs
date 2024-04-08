use std::borrow::Cow;

use gpui::*;
use helix_core::{
    graphemes::ensure_grapheme_boundary_next_byte,
    ropey::RopeSlice,
    syntax::{Highlight, HighlightEvent},
};
use helix_term::keymap::Keymaps;
use helix_view::{graphics::CursorKind, DocumentId, Editor};

pub struct Workspace {
    pub editor: Model<Editor>,
    pub keymaps: Model<Keymaps>,
}

// impl Workspace {
//     fn open_file(&mut self, action: &OpenFile, cx: &mut ViewContext<Self>) {
//         eprintln!("OPEN FILE");
//     }
// }

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

struct DocumentView {
    editor: Model<Editor>,
    keymaps: Model<Keymaps>,
    doc: DocumentId,
    style: TextStyle,
    interactivity: Interactivity,
    focus: FocusHandle,
}

impl DocumentView {
    fn new(
        editor: Model<Editor>,
        keymaps: Model<Keymaps>,
        doc: DocumentId,
        style: TextStyle,
        focus: &FocusHandle,
    ) -> Self {
        Self {
            editor,
            keymaps,
            doc,
            style,
            interactivity: Interactivity::default(),
            focus: focus.clone(),
        }
        .track_focus(&focus)
        .element
    }
}

impl InteractiveElement for DocumentView {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for DocumentView {}

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

impl DocumentView {
    fn highlights(&self, cx: &mut ElementContext) -> Highlights {
        let editor = self.editor.read(cx);
        let doc = editor.document(self.doc).unwrap();
        let text = doc.text().slice(..);
        match doc.syntax() {
            Some(syn) => {
                let iter = syn
                    // TODO: range doesn't actually restrict source, just highlight range
                    .highlight_iter(text.slice(..), None /* todo */, None)
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
            None => Highlights(vec![]),
        }
    }
}

impl IntoElement for DocumentView {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

fn color_to_hsla(color: helix_view::graphics::Color) -> Hsla {
    use helix_view::graphics::Color;
    match color {
        Color::White => hsla(0., 0., 1., 1.),
        Color::Rgb(r, g, b) => {
            let r = (r as u32) << 16;
            let g = (g as u32) << 8;
            let b = b as u32;
            rgb(r | g | b).into()
        }
        _ => todo!(),
    }
}

fn translate_key(ks: &Keystroke) -> helix_view::input::KeyEvent {
    use helix_view::keyboard::{KeyCode, KeyModifiers};

    let mut modifiers = KeyModifiers::NONE;
    if ks.modifiers.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if ks.modifiers.control {
        modifiers |= KeyModifiers::CONTROL;
    }
    if ks.modifiers.shift {
        modifiers |= KeyModifiers::SHIFT;
    }
    let code = match ks.key.as_str() {
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "tab" => KeyCode::Tab,
        "escape" => KeyCode::Esc,
        /* TODO */
        _ => {
            let chars: Vec<char> = ks.key.clone().chars().collect();
            if chars.len() == 1 {
                KeyCode::Char(chars[0])
            } else {
                todo!()
            }
        }
    };

    helix_view::input::KeyEvent { code, modifiers }
}

#[derive(Debug)]
struct DocumentLayout {
    rows: usize,
    columns: usize,
    line_height: Pixels,
    font_size: Pixels,
    cell_width: Pixels,
    hitbox: Option<Hitbox>,
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
        println!("{:?} {:?} {:?}", self.doc, after_layout, bounds);
        let highlights = self.highlights(cx);

        self.interactivity.capture_key_down(|ev, cx| {
            println!("inter {:?}", ev);
        });

        let mode = {
            let editor = self.editor.read(cx);
            editor.mode()
        };
        let focus = self.focus.clone();
        self.interactivity
            .on_mouse_down(MouseButton::Left, move |ev, cx| {
                println!("MOUSE DOWN");
                cx.focus(&focus);
            });

        self.interactivity
            .paint(bounds, after_layout.hitbox.as_ref(), cx, |_, cx| {
                cx.focus(&self.focus);
                let keymaps = self.keymaps.clone();
                let editor = self.editor.clone();
                // // println!("{:?}", highlights);
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
                        let mut ctx = helix_term::commands::Context {
                            editor,
                            register: None,
                            count: None,
                            callback: Vec::new(),
                            on_next_key_callback: None,
                            jobs: &mut helix_term::job::Jobs::new(),
                        };
                        let res = handle_key_result(mode, &mut ctx, res);
                        cx.notify();
                        cx.emit(crate::Update);
                        res
                    });
                    println!("res {:?}", res);
                    if let Some(view_id) = cx.parent_view_id() {
                        println!("redraw?");
                        cx.notify(view_id);
                    }
                });
                //}

                let editor = self.editor.read(cx);

                let cursor = editor.cursor();
                let cursor_pos = cursor.0;
                println!("cursor @ {:?}", cursor);

                let theme = &editor.theme;
                let default_style = theme.get("ui.background");
                let bg_color = color_to_hsla(default_style.bg.unwrap());
                let window_style = theme.get("ui.window");
                let border_color = color_to_hsla(window_style.fg.unwrap());
                let cursor_style = theme.get("ui.cursor.primary");
                println!("{:?}", cursor_style);

                let bg = fill(bounds, bg_color);
                let borders = outline(bounds, border_color);

                let fg_color = color_to_hsla(
                    default_style
                        .fg
                        .unwrap_or(helix_view::graphics::Color::White),
                );
                let document = editor.document(self.doc).unwrap();
                let text = document.text();
                let lines = std::cmp::min(after_layout.rows, text.len_lines());
                let mut shaped_lines = Vec::new();

                let mut char_idx = 0;
                let mut cursor_text = None;

                for (line_nr, line) in text.lines().take(lines).enumerate() {
                    let is_cursor_line = cursor_pos.map(|p| p.row == line_nr).unwrap_or(false);
                    if is_cursor_line {
                        if let Some(text) = line.get_char(cursor_pos.map(|p| p.col).unwrap()) {
                            let cursor_bg = cursor_style
                                .bg
                                .map(|fg| color_to_hsla(fg))
                                .unwrap_or(fg_color);

                            let cursor_fg = cursor_style
                                .fg
                                .map(|fg| color_to_hsla(fg))
                                .unwrap_or(fg_color);

                            let run = TextRun {
                                len: 1,
                                font: self.style.font(),
                                color: cursor_fg,
                                background_color: Some(cursor_bg),
                                underline: None,
                                strikethrough: None,
                            };

                            let text = if text == '\n' { ' ' } else { text };

                            let shaped = cx
                                .text_system()
                                .shape_line(text.to_string().into(), after_layout.font_size, &[run]) // todo: runs
                                .unwrap();
                            cursor_text = Some(shaped);
                        }
                    }
                    //println!("string `{}`", line);
                    let len = line.len_chars();
                    let mut runs = vec![];
                    let regions = highlights.get(char_idx, char_idx + len);

                    let mut previous_end = 0;
                    for reg in regions {
                        //println!("region: {:?}", reg);
                        let HighlightRegion { start, end, hl } = reg;
                        let start = start - char_idx;
                        let end = end - char_idx;

                        let style = theme.highlight(hl.0);
                        let fg = style.fg.map(|fg| color_to_hsla(fg));
                        //let bg = style.fg.map(|fg| color_to_hsla(fg));
                        let len = end - start;

                        // reset to default style
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
                    }

                    let str = RopeWrapper(line).into();
                    //println!("runs {:?}", runs);
                    let shaped = cx
                        .text_system()
                        .shape_line(str, after_layout.font_size, &runs) // todo: runs
                        .unwrap();
                    shaped_lines.push(shaped);
                    char_idx += len;
                }

                cx.paint_quad(bg);
                cx.paint_quad(borders);

                let mut origin = bounds.origin;
                origin.x += px(2.);
                origin.y += px(1.);
                for line in shaped_lines {
                    line.paint(origin, after_layout.line_height, cx).unwrap();
                    origin.y += after_layout.line_height;
                }
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
            });
    }
}

struct RopeWrapper<'a>(RopeSlice<'a>);

impl<'a> Into<SharedString> for RopeWrapper<'a> {
    fn into(self) -> SharedString {
        let cow: Cow<'_, str> = self.0.into();
        (cow.to_string().trim_end().to_string()).into() // this is crazy
    }
}

impl Render for Workspace {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let focus_handle = cx.focus_handle();
        let editor = self.editor.read(cx);
        let default_style = editor.theme.get("ui.background");
        let bg_color = color_to_hsla(default_style.bg.unwrap());

        let mut docs = vec![];
        for (view, _is_focused) in editor.tree.views() {
            let doc = editor.document(view.doc).unwrap();
            let id = doc.id();
            let style = TextStyle {
                font_family: "JetBrains Mono".into(),
                font_size: px(14.0).into(),
                ..Default::default()
            };

            let doc_view = DocumentView::new(
                self.editor.clone(),
                self.keymaps.clone(),
                id,
                style,
                &focus_handle,
            );
            docs.push(doc_view);
        }

        let top_bar = div().w_full().flex().flex_none().h_8();

        eprintln!("rendering workspace");
        div()
            .id("workspace")
            .bg(bg_color)
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .focusable()
            .child(top_bar)
            .children(docs)
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
