use gpui::{rgb, HighlightStyle, Hsla, Keystroke, SharedString, StyledText, TextStyle};
use tui::buffer::Buffer;

pub fn color_to_hsla(color: helix_view::graphics::Color) -> Option<Hsla> {
    use gpui::{black, blue, green, red, white, yellow};
    use helix_view::graphics::Color;
    match color {
        Color::White => Some(white()),
        Color::Black => Some(black()),
        Color::Blue => Some(blue()),
        Color::Green => Some(green()),
        Color::Red => Some(red()),
        Color::Yellow => Some(yellow()),
        Color::Rgb(r, g, b) => {
            let r = (r as u32) << 16;
            let g = (g as u32) << 8;
            let b = b as u32;
            Some(rgb(r | g | b).into())
        }
        Color::Reset => None,
        any => todo!("{:?} not implemented", any),
    }
}

pub fn translate_key(ks: &Keystroke) -> helix_view::input::KeyEvent {
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
    let key = ks.ime_key.as_ref().unwrap_or(&ks.key);
    let code = match key.as_str() {
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "tab" => KeyCode::Tab,
        "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        /* TODO */
        any => {
            let chars: Vec<char> = key.chars().collect();
            if chars.len() == 1 {
                KeyCode::Char(chars[0])
            } else {
                todo!("{:?} key not implemented yet", any)
            }
        }
    };

    helix_view::input::KeyEvent { code, modifiers }
}

/// Handle events by looking them up in `self.keymaps`. Returns None
/// if event was handled (a command was executed or a subkeymap was
/// activated). Only KeymapResult::{NotFound, Cancelled} is returned
/// otherwise.
#[allow(unused)]
pub fn handle_key_result(
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

#[derive(Debug, Clone)]
pub struct TextWithStyle {
    text: SharedString,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
}

impl TextWithStyle {
    pub fn from_buffer(buf: Buffer) -> Self {
        let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();

        let mut text = String::new();
        let rect = buf.area;

        for y in 0..rect.height {
            let mut line = String::new();
            for x in 0..rect.width {
                let cell = &buf[(x, y)];
                let bg = crate::utils::color_to_hsla(cell.bg);
                let fg = crate::utils::color_to_hsla(cell.fg);
                let new_style = HighlightStyle {
                    color: fg,
                    background_color: bg,
                    ..Default::default()
                };
                let length = cell.symbol.len();
                let new_range = if let Some((range, current_highlight)) = highlights.last_mut() {
                    if &new_style == current_highlight {
                        range.end += length;
                        None
                    } else {
                        let range = range.end..range.end + length;
                        Some(range)
                    }
                } else {
                    let range = 0..length;
                    Some(range)
                };
                if let Some(new_range) = new_range {
                    highlights.push((new_range, new_style));
                }
                line.push_str(&cell.symbol);
            }
            if line.chars().all(|c| c == ' ') {
                let mut hl_is_empty = false;
                if let Some(hl) = highlights.last_mut() {
                    hl.0.end -= line.len();
                    hl_is_empty = hl.0.end == hl.0.start;
                }
                if hl_is_empty {
                    highlights.pop();
                }
                continue;
            } else {
                text.push_str(&line);
                text.push_str("\n");
                if let Some(hl) = highlights.last_mut() {
                    hl.0.end += 1; // new line
                }
            }
        }

        TextWithStyle {
            text: text.into(),
            highlights,
        }
    }

    pub fn into_styled_text(self, default_style: &TextStyle) -> StyledText {
        StyledText::new(self.text).with_highlights(default_style, self.highlights)
    }

    pub fn style(&self, idx: usize) -> Option<&HighlightStyle> {
        self.highlights.get(idx).map(|(_, style)| style)
    }
}

pub fn load_tutor(editor: &mut helix_view::editor::Editor) -> Result<(), anyhow::Error> {
    use helix_core::{pos_at_coords, Position, Selection};
    use helix_view::doc_mut;
    use helix_view::editor::Action;
    use std::path::Path;

    let path = helix_loader::runtime_file(Path::new("tutor"));
    // let path = Path::new("./test.rs");
    let doc_id = editor.open(&path, Action::VerticalSplit)?;
    let view_id = editor.tree.focus;
    let doc = doc_mut!(editor, &doc_id);
    let pos = Selection::point(pos_at_coords(
        doc.text().slice(..),
        Position::new(0, 0),
        true,
    ));
    doc.set_selection(view_id, pos);

    // Unset path to prevent accidentally saving to the original tutor file.
    doc_mut!(editor).set_path(None);

    Ok(())
}
