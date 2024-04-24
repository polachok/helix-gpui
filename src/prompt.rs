use gpui::*;
use log::debug;

#[derive(Debug, Clone)]
pub struct Prompt {
    pub text: String,
    pub styles: Vec<(std::ops::Range<usize>, HighlightStyle)>,
}

impl Prompt {
    pub fn make(editor: &mut helix_view::Editor, prompt: &helix_term::ui::Prompt) -> Prompt {
        let area = editor.tree.area();
        let compositor_rect = helix_view::graphics::Rect {
            x: 0,
            y: 0,
            width: area.width * 2 / 3,
            height: area.height,
        };

        let mut comp_ctx = helix_term::compositor::Context {
            editor,
            scroll: None,
            jobs: &mut helix_term::job::Jobs::new(),
        };
        let mut buf = tui::buffer::Buffer::empty(compositor_rect);
        prompt.render_prompt(compositor_rect, &mut buf, &mut comp_ctx);
        let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();

        let mut text = String::new();
        for y in 0..compositor_rect.height {
            let mut line = String::new();
            for x in 0..compositor_rect.width {
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
        Prompt {
            text,
            styles: highlights,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(IntoElement)]
pub struct PromptElement {
    pub prompt: Prompt,
    pub focus: FocusHandle,
}

impl RenderOnce for PromptElement {
    fn render(self, cx: &mut WindowContext) -> impl IntoElement {
        debug!(
            "HIGHLIGHTS {:?} text: `{:?}`",
            self.prompt.styles, self.prompt.text
        );

        let bg_color = self
            .prompt
            .styles
            .get(0)
            .and_then(|(_, style)| style.background_color);
        let mut default_style = TextStyle::default();
        default_style.font_family = "JetBrains Mono".into();
        default_style.font_size = px(12.).into();
        default_style.background_color = bg_color;

        let text =
            StyledText::new(self.prompt.text).with_highlights(&default_style, self.prompt.styles);
        cx.focus(&self.focus);
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .p_5()
            .bg(bg_color.unwrap_or(black()))
            .shadow_sm()
            .rounded_sm()
            .text_color(hsla(1., 1., 1., 1.))
            .font("JetBrains Mono")
            .text_size(px(12.))
            .line_height(px(1.3) * px(12.))
            .child(text)
    }
}
