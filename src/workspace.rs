use gpui::*;
use helix_term::keymap::Keymaps;
use helix_view::Editor;

use crate::document::DocumentView;
use crate::statusline::StatusLine;

pub struct Workspace {
    pub editor: Model<Editor>,
    pub keymaps: Model<Keymaps>,
    pub handle: tokio::runtime::Handle,
}

// impl Workspace {
//     fn open_file(&mut self, action: &OpenFile, cx: &mut ViewContext<Self>) {
//         eprintln!("OPEN FILE");
//     }
// }

impl Render for Workspace {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let focus_handle = cx.focus_handle();
        let editor = self.editor.read(cx);
        let default_style = editor.theme.get("ui.background");
        let default_ui_text = editor.theme.get("ui.text");
        let bg_color = crate::utils::color_to_hsla(default_style.bg.unwrap());
        let text_color = crate::utils::color_to_hsla(default_ui_text.fg.unwrap());

        let mut docs = vec![];
        let mut focused_file_name = None;

        for (view, is_focused) in editor.tree.views() {
            let doc = editor.document(view.doc).unwrap();
            let doc_id = doc.id();
            let view_id = view.id;

            if is_focused {
                focused_file_name = doc.path();
            }

            let style = TextStyle {
                font_family: "JetBrains Mono".into(),
                font_size: px(14.0).into(),
                ..Default::default()
            };

            let doc_elem = DocumentView::new(
                self.editor.clone(),
                self.keymaps.clone(),
                doc_id,
                view_id,
                style.clone(),
                &focus_handle,
                is_focused,
                self.handle.clone(),
            );
            // let style = TextStyle {
            //     font_family: "SF Pro".into(),
            //     font_size: px(12.).into(),
            //     ..Default::default()
            // };
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

        eprintln!("rendering workspace");

        div()
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
                    println!("open file");
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
            cx.update(move |cx| {
                editor.update(cx, move |editor, cx| {
                    let path = &path[0];
                    println!("PATH IS {}", path.display());
                    let _guard = handle.enter();
                    editor.open(path, Action::Replace);
                })
            });
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
