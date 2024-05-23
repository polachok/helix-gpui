use std::collections::{HashMap, HashSet};

use gpui::prelude::FluentBuilder;
use gpui::*;
use helix_term::compositor::Compositor;
use helix_term::job::Jobs;
use helix_term::ui::EditorView;
use helix_view::ViewId;
use log::{debug, info};

use crate::document::DocumentView;
use crate::info_box::InfoBoxView;
use crate::notification::NotificationView;
use crate::overlay::OverlayView;
use crate::prompt::Prompt;
use crate::utils;
use crate::EditorModel;

pub struct Workspace {
    editor: Model<EditorModel>,
    view: Model<EditorView>,
    jobs: Model<Jobs>,
    focused_view_id: Option<ViewId>,
    documents: HashMap<ViewId, View<DocumentView>>,
    compositor: Model<Compositor>,
    handle: tokio::runtime::Handle,
    overlay: View<OverlayView>,
    info: View<InfoBoxView>,
    info_hidden: bool,
    notifications: View<NotificationView>,
}

impl Workspace {
    pub fn new(
        editor: Model<EditorModel>,
        view: Model<EditorView>,
        compositor: Model<Compositor>,
        handle: tokio::runtime::Handle,
        cx: &mut ViewContext<Self>,
    ) -> Self {
        let notifications = Self::init_notifications(&editor, cx);
        let info = Self::init_info_box(&editor, cx);
        let overlay = cx.new_view(|cx| {
            let view = OverlayView::new(&cx.focus_handle());
            view.subscribe(&editor, cx);
            view
        });
        let jobs = cx.new_model(|_| Jobs::new());

        Self {
            editor,
            view,
            focused_view_id: None,
            compositor,
            handle,
            jobs,
            overlay,
            info,
            info_hidden: true,
            documents: HashMap::default(),
            notifications,
        }
    }

    fn init_notifications(
        editor: &Model<EditorModel>,
        cx: &mut ViewContext<Self>,
    ) -> View<NotificationView> {
        let theme = Self::theme(&editor, cx);
        let text_style = theme.get("ui.text.info");
        let popup_style = theme.get("ui.popup.info");
        let popup_bg_color = utils::color_to_hsla(popup_style.bg.unwrap()).unwrap_or(black());
        let popup_text_color = utils::color_to_hsla(text_style.fg.unwrap()).unwrap_or(white());

        cx.new_view(|cx| {
            let view = NotificationView::new(popup_bg_color, popup_text_color);
            view.subscribe(&editor, cx);
            view
        })
    }

    fn init_info_box(editor: &Model<EditorModel>, cx: &mut ViewContext<Self>) -> View<InfoBoxView> {
        let theme = Self::theme(editor, cx);
        let text_style = theme.get("ui.text.info");
        let popup_style = theme.get("ui.popup.info");
        let fg = text_style
            .fg
            .and_then(utils::color_to_hsla)
            .unwrap_or(white());
        let bg = popup_style
            .bg
            .and_then(utils::color_to_hsla)
            .unwrap_or(black());
        let mut style = Style::default();
        style.text.color = Some(fg);
        style.background = Some(bg.into());

        let info = cx.new_view(|cx| {
            let view = InfoBoxView::new(style, &cx.focus_handle());
            view.subscribe(&editor, cx);
            view
        });
        cx.subscribe(&info, |v, _e, _evt, cx| {
            v.info_hidden = true;
            cx.notify();
        })
        .detach();
        info
    }

    pub fn theme(editor: &Model<EditorModel>, cx: &mut ViewContext<Self>) -> helix_view::Theme {
        editor.read(cx).theme()
    }

    pub fn handle_event(&mut self, ev: &crate::Update, cx: &mut ViewContext<Self>) {
        info!("handling event {:?}", ev);
        match ev {
            crate::Update::EditorEvent(ev) => {
                use helix_view::editor::EditorEvent;
                match ev {
                    EditorEvent::Redraw => cx.notify(),
                    EditorEvent::LanguageServerMessage(_) => { /* handled by notifications */ }
                    _ => {
                        info!("editor event {:?} not handled", ev);
                    }
                }
            }
            crate::Update::EditorStatus(_) => {}
            crate::Update::Redraw => {
                cx.notify();
            }
            crate::Update::Prompt(_) | crate::Update::Picker(_) => {
                // handled by overlay
                cx.notify();
            }
            crate::Update::Info(_) => {
                self.info_hidden = false;
                // handled by the info box view
            }
        }
    }

    fn render_tree(
        root_id: ViewId,
        root: Div,
        containers: &mut HashMap<ViewId, Div>,
        tree: &HashMap<ViewId, Vec<ViewId>>,
    ) -> Div {
        let mut root = root;
        if let Some(children) = tree.get(&root_id) {
            for child_id in children {
                let child = containers.remove(child_id).unwrap();
                let child = Self::render_tree(*child_id, child, containers, tree);
                root = root.child(child);
            }
        }
        root
    }

    fn handle_key(&mut self, ev: &KeyDownEvent, cx: &mut ViewContext<Self>) {
        println!("WORKSPACE KEY DOWN: {:?}", ev.keystroke);

        let editor = self.editor.clone();
        let jobs = self.jobs.clone();
        let compositor = self.compositor.clone();
        let rt_handle = self.handle.clone();
        let view = self.view.clone();

        let key = utils::translate_key(&ev.keystroke);

        editor.update(cx, |editor, cx| {
            let _guard = rt_handle.enter();

            let is_handled = jobs.update(cx, |mut jobs, cx| {
                compositor.update(cx, |compositor, cx| {
                    let mut editor = editor.lock();
                    let mut comp_ctx = helix_term::compositor::Context {
                        editor: &mut editor,
                        scroll: None,
                        jobs: &mut jobs,
                    };
                    let mut is_handled =
                        compositor.handle_event(&helix_view::input::Event::Key(key), &mut comp_ctx);
                    debug!("is handled by comp? {:?}", is_handled);

                    if !is_handled {
                        is_handled = view.update(cx, |view, _cx| {
                            use helix_term::compositor::{Component, EventResult};
                            let event = &helix_view::input::Event::Key(key);
                            let res = view.handle_event(event, &mut comp_ctx);
                            let is_handled = matches!(res, EventResult::Consumed(_));
                            if let EventResult::Consumed(Some(cb)) = res {
                                cb(compositor, &mut comp_ctx);
                            }
                            is_handled
                        });
                    }
                    is_handled
                })
            });
            debug!("is handled? {:?}", is_handled);
            println!("KEY IS HANDLED ? {:?}", is_handled);

            let (prompt, picker) = compositor.update(cx, |compositor, _cx| {
                use crate::picker::Picker as PickerComponent;
                use helix_term::ui::{overlay::Overlay, Picker};
                use std::path::PathBuf;
                let mut editor = editor.lock();

                let picker = if let Some(p) =
                    compositor.find_id::<Overlay<Picker<PathBuf>>>(helix_term::ui::picker::ID)
                {
                    println!("found file picker");
                    Some(PickerComponent::make(&mut editor, &mut p.content))
                } else {
                    None
                };
                let prompt = if let Some(p) = compositor.find::<helix_term::ui::Prompt>() {
                    Some(Prompt::make(&mut editor, p))
                } else {
                    None
                };

                (prompt, picker)
            });

            if let Some(picker) = picker {
                cx.emit(crate::Update::Picker(picker));
            }

            if let Some(prompt) = prompt {
                cx.emit(crate::Update::Prompt(prompt));
            }

            if let Some(info) = editor.lock().autoinfo.take() {
                cx.emit(crate::Update::Info(info));
            }
            drop(_guard);
        });
        if let Some(view) = self.focused_view_id.and_then(|id| self.documents.get(&id)) {
            view.update(cx, |_view, cx| {
                cx.notify();
            })
        }
        cx.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx).clone();
        let editor = editor.lock();

        let default_style = editor.theme.get("ui.background");
        let default_ui_text = editor.theme.get("ui.text");
        let bg_color = utils::color_to_hsla(default_style.bg.unwrap()).unwrap_or(black());
        let text_color = utils::color_to_hsla(default_ui_text.fg.unwrap()).unwrap_or(white());
        let window_style = editor.theme.get("ui.window");
        let border_color = utils::color_to_hsla(window_style.fg.unwrap()).unwrap_or(white());

        let editor_rect = editor.tree.area();

        let mut focused_file_name = None;
        let mut view_ids = HashSet::new();
        let mut right_borders = HashSet::new();

        for (view, is_focused) in editor.tree.views() {
            let doc = editor.document(view.doc).unwrap();
            let view_id = view.id;

            if editor
                .tree
                .find_split_in_direction(view_id, helix_view::tree::Direction::Right)
                .is_some()
            {
                right_borders.insert(view_id);
            }

            view_ids.insert(view_id);

            if is_focused {
                self.focused_view_id = Some(view_id);
                focused_file_name = doc.path().map(|p| p.display().to_string());
            }

            let style = TextStyle {
                font_family: cx.global::<crate::FontSettings>().fixed_font.family.clone(),
                font_size: px(14.0).into(),
                ..Default::default()
            };

            let view = self.documents.entry(view_id).or_insert_with(|| {
                cx.new_view(|cx| {
                    DocumentView::new(
                        self.editor.clone(),
                        view_id,
                        style.clone(),
                        &cx.focus_handle(),
                        is_focused,
                    )
                })
            });
            view.update(cx, |view, _cx| {
                view.set_focused(is_focused);
            });
        }
        use helix_view::tree::{ContainerItem, Layout};
        let mut containers = HashMap::new();
        let mut tree = HashMap::new();
        let mut root_id = None;

        for item in editor.tree.traverse_containers() {
            match item {
                ContainerItem::Container { id, parent, layout } => {
                    let container = match layout {
                        Layout::Horizontal => div().flex().size_full().flex_col(),
                        Layout::Vertical => div().flex().size_full().flex_row(),
                    };
                    containers.insert(id, container);
                    let entry = tree.entry(parent).or_insert_with(|| Vec::new());

                    if id == parent {
                        root_id = Some(id);
                    } else {
                        entry.push(id);
                    }
                }
                ContainerItem::Child { id, parent } => {
                    let view = self.documents.get(&id).unwrap().clone();
                    let has_border = right_borders.contains(&id);
                    let view = div()
                        .flex()
                        .size_full()
                        .child(view)
                        .when(has_border, |this| {
                            this.border_color(border_color).border_r_1()
                        });

                    let mut container = containers.remove(&parent).unwrap();
                    container = container.child(view);
                    containers.insert(parent, container);
                }
            }
        }

        drop(editor);
        let to_remove = self
            .documents
            .keys()
            .copied()
            .filter(|id| !view_ids.contains(id))
            .collect::<Vec<_>>();
        for view_id in to_remove {
            if let Some(view) = self.documents.remove(&view_id) {
                cx.dismiss_view(&view);
            }
        }

        let mut docs_root = None;
        // println!("containers: {:?} tree: {:?}", containers.len(), tree);
        if let Some(root_id) = root_id {
            let root = containers.remove(&root_id).unwrap();
            let child = Self::render_tree(root_id, root, &mut containers, &tree);
            let root = div().flex().w_full().h_full().child(child);
            docs_root = Some(root);
        }
        // docs.push(root);
        // for view in self.documents.values() {
        //     docs.push(AnyView::from(view.clone()).cached(StyleRefinement::default().size_full()));
        // }

        let focused_view = self
            .focused_view_id
            .and_then(|id| self.documents.get(&id))
            .cloned();
        if let Some(view) = &focused_view {
            cx.focus_view(view);
        }

        let label = if let Some(path) = focused_file_name {
            div()
                .flex_shrink()
                .font(cx.global::<crate::FontSettings>().var_font.clone())
                .text_color(text_color)
                .text_size(px(12.))
                .child(format!("{} - Helix", path))
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

        println!("rendering workspace");

        let compositor = self.compositor.clone();

        compositor.update(cx, move |compositor, _cx| {
            compositor.resize(editor_rect);
        });

        if let Some(view) = &focused_view {
            cx.focus_view(view);
        }

        div()
            .on_key_down(cx.listener(|view, ev, cx| {
                view.handle_key(ev, cx);
            }))
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
            .on_action(move |&crate::Hide, cx| cx.hide())
            .on_action(move |&crate::HideOthers, cx| cx.hide_other_apps())
            .on_action(move |&crate::ShowAll, cx| cx.unhide_other_apps())
            .on_action(move |&crate::Minimize, cx| cx.minimize_window())
            .on_action(move |&crate::Zoom, cx| cx.zoom_window())
            .on_action({
                let handle = self.handle.clone();
                let editor = self.editor.clone();
                cx.listener(move |_, &crate::Tutor, cx| {
                    load_tutor(editor.clone(), handle.clone(), cx)
                })
            })
            .id("workspace")
            .bg(bg_color)
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .focusable()
            .child(top_bar)
            .when_some(docs_root, |this, docs| this.child(docs))
            .child(self.notifications.clone())
            .when(!self.overlay.read(cx).is_empty(), |this| {
                let view = &self.overlay;
                cx.focus_view(&view);
                this.child(view.clone())
            })
            .when(
                !self.info_hidden && !self.info.read(cx).is_empty(),
                |this| {
                    let info = &self.info;
                    cx.focus_view(&info);
                    this.child(info.clone())
                },
            )
    }
}

fn load_tutor(
    editor: Model<EditorModel>,
    handle: tokio::runtime::Handle,
    cx: &mut ViewContext<Workspace>,
) {
    let _guard = handle.enter();
    let mut editor = editor.read(cx).lock();
    let _ = utils::load_tutor(&mut editor);
    drop(editor);
    cx.notify()
}

fn open(editor: Model<EditorModel>, handle: tokio::runtime::Handle, cx: &mut WindowContext) {
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
                    let mut editor = editor.lock();
                    editor.open(path, Action::Replace).unwrap();
                })
            })
            .unwrap();
        }
    })
    .detach();
}

fn quit(editor: Model<EditorModel>, rt: tokio::runtime::Handle, cx: &mut WindowContext) {
    editor.update(cx, |editor, _cx| {
        let mut editor = editor.lock();
        let _guard = rt.enter();
        rt.block_on(async { editor.flush_writes().await }).unwrap();
        let views: Vec<_> = editor.tree.views().map(|(view, _)| view.id).collect();
        for view_id in views {
            editor.close(view_id);
        }
    });
}
