use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::picker::{Picker, PickerElement};
use crate::prompt::{Prompt, PromptElement};

pub struct OverlayView {
    prompt: Option<Prompt>,
    picker: Option<Picker>,
    focus: FocusHandle,
}

impl OverlayView {
    pub fn new(focus: &FocusHandle) -> Self {
        Self {
            prompt: None,
            picker: None,
            focus: focus.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.prompt.is_none() && self.picker.is_none()
    }

    pub fn subscribe(&self, editor: &Model<crate::EditorModel>, cx: &mut ViewContext<Self>) {
        cx.subscribe(editor, |this, _, ev, cx| {
            this.handle_event(ev, cx);
        })
        .detach()
    }

    fn handle_event(&mut self, ev: &crate::Update, cx: &mut ViewContext<Self>) {
        match ev {
            crate::Update::Prompt(prompt) => {
                self.prompt = Some(prompt.clone());
                cx.notify();
            }
            crate::Update::Picker(picker) => {
                self.picker = Some(picker.clone());
                cx.notify();
            }
            _ => {}
        }
    }
}

impl FocusableView for OverlayView {
    fn focus_handle(&self, _cx: &AppContext) -> FocusHandle {
        self.focus.clone()
    }
}
impl EventEmitter<DismissEvent> for OverlayView {}

impl Render for OverlayView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        println!("rendering overlay");
        div().absolute().size_full().bottom_0().left_0().child(
            div()
                .flex()
                .h_full()
                .justify_center()
                .items_center()
                .when_some(self.prompt.take(), |this, prompt| {
                    let handle = cx.focus_handle();
                    let prompt = PromptElement {
                        prompt,
                        focus: handle.clone(),
                    };
                    handle.focus(cx);
                    this.child(prompt)
                })
                .when_some(self.picker.take(), |this, picker| {
                    let handle = cx.focus_handle();
                    let picker = PickerElement {
                        picker,
                        focus: handle.clone(),
                    };
                    handle.focus(cx);
                    this.child(picker)
                }),
        )
    }
}
