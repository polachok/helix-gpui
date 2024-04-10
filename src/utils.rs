use gpui::{hsla, rgb, Hsla, Keystroke};

pub fn color_to_hsla(color: helix_view::graphics::Color) -> Hsla {
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
