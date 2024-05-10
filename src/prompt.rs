use gpui::*;

use crate::utils::TextWithStyle;

#[derive(Debug, Clone)]
pub struct Prompt(TextWithStyle);

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
        Prompt(TextWithStyle::from_buffer(buf))
    }
}

#[derive(IntoElement)]
pub struct PromptElement {
    pub prompt: Prompt,
    pub focus: FocusHandle,
}

impl RenderOnce for PromptElement {
    fn render(self, cx: &mut WindowContext) -> impl IntoElement {
        let bg_color = self
            .prompt
            .0
            .style(0)
            .and_then(|style| style.background_color);
        let mut default_style = TextStyle::default();
        default_style.font_family = "JetBrains Mono".into();
        default_style.font_size = px(12.).into();
        default_style.background_color = bg_color;

        let text = self.prompt.0.into_styled_text(&default_style);
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
            .font(cx.global::<crate::FontSettings>().fixed_font.clone())
            .text_size(px(12.))
            .line_height(px(1.3) * px(12.))
            .child(text)
    }
}
