use gpui::*;
use helix_term::keymap::Keymaps;
use helix_view::Editor;

use crate::document::DocumentView;

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
        let bg_color = crate::utils::color_to_hsla(default_style.bg.unwrap());

        let mut docs = vec![];
        for (view, is_focused) in editor.tree.views() {
            let doc = editor.document(view.doc).unwrap();
            let doc_id = doc.id();
            let view_id = view.id;
            let style = TextStyle {
                font_family: "JetBrains Mono".into(),
                font_size: px(14.0).into(),
                ..Default::default()
            };

            let doc_view = DocumentView::new(
                self.editor.clone(),
                self.keymaps.clone(),
                doc_id,
                view_id,
                style,
                &focus_handle,
                is_focused,
                self.handle.clone(),
            );
            docs.push(doc_view);
        }

        let top_bar = div().w_full().flex().flex_none().h_8();

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
