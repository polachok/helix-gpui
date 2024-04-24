use gpui::prelude::FluentBuilder;
use gpui::*;
use helix_term::compositor::Compositor;
use helix_term::keymap::Keymaps;
use helix_view::Editor;
use log::{debug, info};

use crate::document::DocumentView;
use crate::info_box::InfoBox;
use crate::prompt::{Prompt, PromptElement};
use crate::statusline::StatusLine;

pub struct Workspace {
    editor: Model<Editor>,
    keymaps: Model<Keymaps>,
    compositor: Model<Compositor>,
    handle: tokio::runtime::Handle,
    prompt: Option<Prompt>,
    info: Option<InfoBox>,
}

impl Workspace {
    pub fn new(
        editor: Model<Editor>,
        keymaps: Model<Keymaps>,
        compositor: Model<Compositor>,
        handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            editor,
            keymaps,
            compositor,
            handle,
            prompt: None,
            info: None,
        }
    }

    pub fn handle_event(&mut self, ev: &crate::Update, cx: &mut ViewContext<Self>) {
        info!("handling editor event {:?}", ev);
        match ev {
            crate::Update::Redraw => {}
            crate::Update::Prompt(prompt) => {
                self.prompt = Some(prompt.clone());
            }
            crate::Update::Info(info) => {
                info!("INFO {:?}", info);
                let editor = self.editor.read(cx);
                let text_style = editor.theme.get("ui.text.info");
                let popup_style = editor.theme.get("ui.popup.info");
                let fg = text_style
                    .fg
                    .and_then(crate::utils::color_to_hsla)
                    .unwrap_or(white());
                let bg = popup_style
                    .bg
                    .and_then(crate::utils::color_to_hsla)
                    .unwrap_or(black());
                let mut style = Style::default();
                style.text.color = Some(fg);
                style.background = Some(bg.into());

                self.info = Some(InfoBox::new(info, style));
            }
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let focus_handle = cx.focus_handle();
        let editor = self.editor.read(cx);
        let default_style = editor.theme.get("ui.background");
        let default_ui_text = editor.theme.get("ui.text");
        let bg_color = crate::utils::color_to_hsla(default_style.bg.unwrap()).unwrap_or(black());
        let text_color =
            crate::utils::color_to_hsla(default_ui_text.fg.unwrap()).unwrap_or(white());

        let mut docs = vec![];
        let mut focused_file_name = None;
        let mut focused_doc = None;

        let editor_rect = editor.tree.area();

        let mut focused_view_id = None;

        for (view, is_focused) in editor.tree.views() {
            let doc = editor.document(view.doc).unwrap();
            let doc_id = doc.id();
            let view_id = view.id;
            let focus_handle = focus_handle.clone();

            if is_focused {
                focused_view_id = Some(view_id);
                focused_file_name = doc.path();
                focused_doc = Some(focus_handle.clone());
            }

            let style = TextStyle {
                font_family: "JetBrains Mono".into(),
                font_size: px(14.0).into(),
                ..Default::default()
            };

            let doc_elem = DocumentView::new(
                self.editor.clone(),
                self.keymaps.clone(),
                self.compositor.clone(),
                doc_id,
                view_id,
                style.clone(),
                &focus_handle,
                is_focused,
                self.handle.clone(),
            );
            let status = StatusLine::new(
                self.editor.clone(),
                doc_id,
                view_id,
                is_focused,
                style.clone(),
            );

            let view = div()
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .child(doc_elem)
                .child(status);
            docs.push(view);
        }

        let label = if let Some(path) = focused_file_name {
            div()
                .flex_shrink()
                .font("SF Pro")
                .text_color(text_color)
                .text_size(px(12.))
                .child(format!("{} - Helix", path.display()))
        } else {
            div().flex()
        };
        let top_bar = div()
            .w_full()
            .flex()
            .flex_none()
            .h_8()
            .justify_center()
            .items_center()
            .child(label);

        debug!("rendering workspace");

        if let Some(handle) = focused_doc {
            if !handle.is_focused(cx) {
                println!("FOCUSING {:?}", handle);
                handle.focus(cx);
            }
        }

        let has_prompt = self.prompt.is_some();
        let prompt = div().absolute().size_full().bottom_0().left_0().child(
            div()
                .absolute()
                .flex()
                .left_1()
                .bottom_7()
                .flex_col()
                .items_start()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_end()
                        .when(has_prompt, |this| {
                            let handle = cx.focus_handle();
                            let prompt = PromptElement {
                                prompt: self.prompt.take().unwrap(),
                                focus: handle.clone(),
                            };
                            handle.focus(cx);
                            this.child(prompt)
                        }),
                ),
        );

        let keymaps = self.keymaps.clone();
        let editor = self.editor.clone();
        let compositor = self.compositor.clone();
        let rt_handle = self.handle.clone();

        compositor.update(cx, move |compositor, _cx| {
            compositor.resize(editor_rect);
        });

        div()
            .on_key_down(move |ev, cx| {
                println!("WORKSPACE KEY DOWN: {:?}", ev.keystroke);
                let keymaps = keymaps.clone();

                let key = crate::utils::translate_key(&ev.keystroke);

                debug!("WORKSPACE KEY EVENT {:?}", key);

                editor.update(cx, |editor, cx| {
                    let _guard = rt_handle.enter();
                    let mode = editor.mode();

                    let mut ctx = helix_term::commands::Context {
                        editor,
                        register: None,
                        count: None,
                        callback: Vec::new(),
                        on_next_key_callback: None,
                        jobs: &mut helix_term::job::Jobs::new(),
                    };

                    let is_handled = compositor.update(cx, |compositor, _cx| {
                        let mut comp_ctx = helix_term::compositor::Context {
                            editor: ctx.editor,
                            scroll: None,
                            jobs: &mut helix_term::job::Jobs::new(),
                        };
                        let is_handled = compositor
                            .handle_event(&helix_view::input::Event::Key(key), &mut comp_ctx);
                        println!("is handled? {:?}", is_handled);

                        is_handled
                    });

                    if !is_handled {
                        let kmap = keymaps.update(cx, |keymaps, _cx| keymaps.get(mode, key));
                        let res = crate::utils::handle_key_result(mode, &mut ctx, kmap);
                        info!("key result: {:?}", res);
                    }

                    let prompt = if ctx.callback.len() != 0 || is_handled {
                        let callbacks = ctx.callback;
                        let prompt = compositor.update(cx, |compositor, _cx| {
                            Prompt::make(compositor, &mut ctx.editor, callbacks)
                        });

                        if prompt.is_empty() {
                            None
                        } else {
                            Some(prompt)
                        }
                    } else {
                        None
                    };

                    if let Some(info) = editor.autoinfo.take() {
                        cx.emit(crate::Update::Info(info));
                    }

                    if let Some(view_id) = focused_view_id {
                        editor.ensure_cursor_in_view(view_id);
                    }
                    drop(_guard);
                    if let Some(prompt) = prompt {
                        cx.emit(crate::Update::Prompt(prompt));
                    }
                    cx.notify();
                    cx.emit(crate::Update::Redraw);
                });
            })
            .on_action(move |&crate::About, _cx| {
                eprintln!("hello");
            })
            .on_action({
                let handle = self.handle.clone();
                let editor = self.editor.clone();

                move |&crate::Quit, cx| {
                    eprintln!("quit?");
                    quit(editor.clone(), handle.clone(), cx);
                    eprintln!("quit!");
                    cx.quit();
                }
            })
            .on_action({
                let handle = self.handle.clone();
                let editor = self.editor.clone();

                move |&crate::OpenFile, cx| {
                    info!("open file");
                    open(editor.clone(), handle.clone(), cx)
                }
            })
            .id("workspace")
            .bg(bg_color)
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .focusable()
            .child(top_bar)
            .children(docs)
            .when(has_prompt, move |this| this.child(prompt))
            .when(self.info.is_some(), move |this| {
                this.child(self.info.take().unwrap())
            })
    }
}

fn open(editor: Model<Editor>, handle: tokio::runtime::Handle, cx: &mut WindowContext) {
    let path = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
    });
    cx.spawn(move |mut cx| async move {
        if let Ok(Some(path)) = path.await {
            use helix_view::editor::Action;
            // TODO: handle errors
            cx.update(move |cx| {
                editor.update(cx, move |editor, _cx| {
                    let path = &path[0];
                    let _guard = handle.enter();
                    editor.open(path, Action::Replace).unwrap();
                })
            })
            .unwrap();
        }
    })
    .detach();
}

fn quit(editor: Model<Editor>, rt: tokio::runtime::Handle, cx: &mut WindowContext) {
    editor.update(cx, |editor, _cx| {
        let _guard = rt.enter();
        rt.block_on(async { editor.flush_writes().await }).unwrap();
        let views: Vec<_> = editor.tree.views().map(|(view, _)| view.id).collect();
        for view_id in views {
            editor.close(view_id);
        }
    });
}
