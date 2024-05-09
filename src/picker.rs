use gpui::*;

use crate::utils::TextWithStyle;

#[derive(Debug, Clone)]
pub struct Picker(TextWithStyle);

// TODO: this is copy-paste from Prompt, refactor it later
impl Picker {
    pub fn make<T: helix_term::ui::menu::Item>(
        editor: &mut helix_view::Editor,
        prompt: &mut helix_term::ui::Picker<T>,
    ) -> Self {
        use helix_term::compositor::Component;
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
        prompt.render(compositor_rect, &mut buf, &mut comp_ctx);
        Self(TextWithStyle::from_buffer(buf))
    }
}

#[derive(IntoElement)]
pub struct PickerElement {
    pub picker: Picker,
    pub focus: FocusHandle,
}

impl RenderOnce for PickerElement {
    fn render(self, cx: &mut WindowContext) -> impl IntoElement {
        let bg_color = self
            .picker
            .0
            .style(0)
            .and_then(|style| style.background_color);
        let mut default_style = TextStyle::default();
        default_style.font_family = "JetBrains Mono".into();
        default_style.font_size = px(12.).into();
        default_style.background_color = bg_color;

        // println!("picker: {:?}", self.picker.0);
        let text = self.picker.0.into_styled_text(&default_style);
        cx.focus(&self.focus);
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
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
