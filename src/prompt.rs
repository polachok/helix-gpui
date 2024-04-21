use gpui::*;

#[derive(Debug, Clone, IntoElement)]
pub struct Prompt {
    pub text: String,
    pub styles: Vec<(std::ops::Range<usize>, HighlightStyle)>,
}

impl RenderOnce for Prompt {
    fn render(self, _cx: &mut WindowContext) -> impl IntoElement {
        println!("HIGHLIGHTS {:?}", self.styles);

        let bg_color = self.styles[0].1.background_color.unwrap_or(black());
        let mut default_style = TextStyle::default();
        default_style.font_family = "JetBrains Mono".into();

        let text = StyledText::new(self.text).with_highlights(&default_style, self.styles);
        div()
            .flex()
            .flex_col()
            .p_5()
            .border_color(hsla(0., 0., 0., 1.))
            .border_2()
            .bg(bg_color)
            .shadow_lg()
            .text_color(hsla(1., 1., 1., 1.))
            .font("JetBrains Mono")
            .child(text)
    }
}
