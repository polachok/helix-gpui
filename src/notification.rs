use std::collections::HashMap;

use gpui::{prelude::FluentBuilder as _, *};
use helix_lsp::{
    lsp::{NumberOrString, ProgressParamsValue, WorkDoneProgress},
    LanguageServerId,
};
use log::info;

enum LspStatusEvent {
    Begin,
    Progress,
    End,
    Ignore,
}

#[derive(Default, Debug)]
struct LspStatus {
    token: String,
    title: String,
    message: Option<String>,
    percentage: Option<u32>,
}

impl LspStatus {
    fn is_empty(&self) -> bool {
        self.token == "" && self.title == "" && self.message.is_none()
    }
}

#[derive(IntoElement)]
struct Notification {
    title: String,
    message: Option<String>,
    bg: Hsla,
    text: Hsla,
}

impl Notification {
    fn from_lsp(status: &LspStatus, bg: Hsla, text: Hsla) -> Self {
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

pub struct NotificationView {
    lsp_status: HashMap<LanguageServerId, LspStatus>,
    popup_bg_color: Hsla,
    popup_text_color: Hsla,
}

impl NotificationView {
    pub fn new(popup_bg_color: Hsla, popup_text_color: Hsla) -> Self {
        Self {
            lsp_status: HashMap::new(),
            popup_bg_color,
            popup_text_color,
        }
    }

    fn handle_lsp_call(&mut self, id: LanguageServerId, call: &helix_lsp::Call) -> LspStatusEvent {
        use helix_lsp::{Call, Notification};
        let mut ev = LspStatusEvent::Ignore;

        let status = self.lsp_status.entry(id).or_default();

        match call {
            Call::Notification(notification) => {
                if let Ok(notification) =
                    Notification::parse(&notification.method, notification.params.clone())
                {
                    match notification {
                        Notification::ProgressMessage(ref msg) => {
                            let token = match msg.token.clone() {
                                NumberOrString::String(s) => s,
                                NumberOrString::Number(num) => num.to_string(),
                            };
                            status.token = token;
                            let ProgressParamsValue::WorkDone(value) = msg.value.clone();
                            match value {
                                WorkDoneProgress::Begin(begin) => {
                                    status.title = begin.title;
                                    status.message = begin.message;
                                    status.percentage = begin.percentage;
                                    ev = LspStatusEvent::Begin;
                                }
                                WorkDoneProgress::Report(report) => {
                                    if let Some(msg) = report.message {
                                        status.message = Some(msg);
                                    }
                                    status.percentage = report.percentage;

                                    ev = LspStatusEvent::Progress;
                                }
                                WorkDoneProgress::End(end) => {
                                    if let Some(msg) = end.message {
                                        status.message = Some(msg);
                                    }
                                    ev = LspStatusEvent::End;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        // println!("{:?}", status);
        ev
    }

    pub fn subscribe(&self, editor: &Model<crate::EditorModel>, cx: &mut ViewContext<Self>) {
        cx.subscribe(editor, |this, _, ev, cx| {
            this.handle_event(ev, cx);
        })
        .detach()
    }

    fn handle_event(&mut self, ev: &crate::Update, cx: &mut ViewContext<Self>) {
        use helix_view::editor::EditorEvent;

        info!("handling event {:?}", ev);
        if let crate::Update::EditorEvent(EditorEvent::LanguageServerMessage((id, call))) = ev {
            let ev = self.handle_lsp_call(*id, call);
            match ev {
                LspStatusEvent::Begin => {
                    let id = *id;
                    cx.spawn(|this, mut cx| async move {
                        loop {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(5000))
                                .await;
                            this.update(&mut cx, |this, _cx| {
                                if this.lsp_status.contains_key(&id) {
                                    // TODO: this call causes workspace redraw for some reason
                                    //cx.notify();
                                }
                            })
                            .ok();
                        }
                    })
                    .detach();
                }
                LspStatusEvent::Progress => {}
                LspStatusEvent::Ignore => {}
                LspStatusEvent::End => {
                    self.lsp_status.remove(id);
                }
            }
        }
    }
}

impl Render for NotificationView {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        let mut notifications = vec![];
        for status in self.lsp_status.values() {
            if status.is_empty() {
                continue;
            }
            notifications.push(Notification::from_lsp(
                status,
                self.popup_bg_color,
                self.popup_text_color,
            ));
        }
        div()
            .absolute()
            .w(DefiniteLength::Fraction(0.33))
            .top_8()
            .right_5()
            .flex_col()
            .gap_8()
            .justify_start()
            .items_center()
            .children(notifications)
    }
}

impl RenderOnce for Notification {
    fn render(mut self, cx: &mut WindowContext) -> impl IntoElement {
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
            .font(cx.global::<crate::FontSettings>().fixed_font.clone())
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
