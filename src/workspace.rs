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
            .on_action(|&crate::About, _cx| {
                eprintln!("hello");
            })
            .on_action(|&crate::Quit, _cx| eprintln!("quit?"))
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
