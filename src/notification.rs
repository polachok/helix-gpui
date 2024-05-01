use gpui::{prelude::FluentBuilder as _, *};

#[derive(Default, Debug)]
pub struct LspStatus {
    pub token: String,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

impl LspStatus {
    pub fn is_empty(&self) -> bool {
        self.token == "" && self.title == "" && self.message.is_none()
    }
}

#[derive(IntoElement)]
pub struct Notification {
    title: String,
    message: Option<String>,
    bg: Hsla,
    text: Hsla,
}

impl Notification {
    pub fn from_lsp(status: &LspStatus, bg: Hsla, text: Hsla) -> Self {
        let title = format!(
            "{}: {} {}",
            status.token,
            status.title,
            status
                .percentage
                .map(|s| format!("{}%", s))
                .unwrap_or_default()
        );
        Notification {
            title,
            message: status.message.clone(),
            bg,
            text,
        }
    }
}

impl RenderOnce for Notification {
    fn render(mut self, _cx: &mut WindowContext) -> impl IntoElement {
        let message = self.message.take();
        div()
            .flex()
            .flex_col()
            .flex_shrink()
            .p_2()
            .gap_4()
            .min_h(px(100.))
            .bg(self.bg)
            .text_color(self.text)
            .shadow_sm()
            .rounded_sm()
            .font("JetBrains Mono")
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .font_weight(FontWeight::BOLD)
                    .flex_none()
                    .justify_center()
                    .items_center()
                    .child(self.title),
            )
            .when_some(message, |this, msg| this.child(msg))
    }
}
