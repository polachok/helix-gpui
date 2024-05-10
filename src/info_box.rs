use gpui::prelude::FluentBuilder;
use gpui::*;
use helix_view::info::Info;

#[derive(Debug)]
pub struct InfoBoxView {
    title: Option<SharedString>,
    text: Option<SharedString>,
    style: Style,
    focus: FocusHandle,
}

impl InfoBoxView {
    pub fn new(style: Style, focus: &FocusHandle) -> Self {
        InfoBoxView {
            title: None,
            text: None,
            style,
            focus: focus.clone(),
        }
    }

    fn handle_event(&mut self, ev: &crate::Update, cx: &mut ViewContext<Self>) {
        if let crate::Update::Info(info) = ev {
            self.set_info(info);
            cx.notify();
        }
    }

    pub fn subscribe(&self, editor: &Model<crate::EditorModel>, cx: &mut ViewContext<Self>) {
        cx.subscribe(editor, |this, _, ev, cx| {
            this.handle_event(ev, cx);
        })
        .detach()
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_none()
    }

    pub fn set_info(&mut self, info: &Info) {
        self.title = Some(info.title.clone().into());
        self.text = Some(info.text.clone().into());
    }
}

impl FocusableView for InfoBoxView {
    fn focus_handle(&self, _cx: &AppContext) -> FocusHandle {
        self.focus.clone()
    }
}
impl EventEmitter<DismissEvent> for InfoBoxView {}

impl Render for InfoBoxView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let font = cx.global::<crate::FontSettings>().fixed_font.clone();

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_v, _e, cx| {
                println!("INFO BOX received key");
                cx.emit(DismissEvent)
            }))
            .absolute()
            .bottom_7()
            .right_1()
            .flex()
            .flex_row()
            .child(
                div()
                    .rounded_sm()
                    .shadow_sm()
                    .font(font)
                    .text_size(px(12.))
                    .text_color(self.style.text.color.unwrap())
                    .bg(self.style.background.as_ref().cloned().unwrap())
                    .p_2()
                    .flex()
                    .flex_row()
                    .content_end()
                    .when_some(self.title.as_ref(), |this, title| {
                        this.child(
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
                                        .child(title.clone()),
                                )
                                .when_some(self.text.as_ref(), |this, text| {
                                    this.child(text.clone())
                                }),
                        )
                    }),
            )
    }
}
