use gpui::*;
use helix_view::info::Info;

#[derive(Debug, IntoElement)]
pub struct InfoBox {
    title: String,
    text: String,
    style: Style,
}

impl InfoBox {
    pub fn new(info: &Info, style: Style) -> Self {
        InfoBox {
            title: info.title.clone(),
            text: info.text.clone(),
            style,
        }
    }
}

impl RenderOnce for InfoBox {
    fn render(self, _cx: &mut WindowContext) -> impl IntoElement {
        div()
            .absolute()
            .bottom_7()
            .right_1()
            .flex()
            .flex_row()
            .child(
                div()
                    .rounded_sm()
                    .shadow_sm()
                    .font("JetBrains Mono")
                    .text_size(px(12.))
                    .text_color(self.style.text.color.unwrap())
                    .bg(self.style.background.unwrap())
                    .p_2()
                    .flex()
                    .flex_row()
                    .content_end()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .font_weight(FontWeight::BOLD)
                                    .flex_none()
                                    .justify_center()
                                    .items_center()
                                    .child(self.title),
                            )
                            .child(self.text),
                    ),
            )
    }
}
