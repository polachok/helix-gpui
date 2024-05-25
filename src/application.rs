use std::{path::Path, sync::Arc};

use arc_swap::{access::Map, ArcSwap};
use futures_util::stream::Stream;
use helix_core::diagnostic::Severity;
use helix_core::{pos_at_coords, syntax, Position, Selection};

use helix_stdx::path::get_relative_path;
use helix_term::job::Jobs;
use helix_term::{
    args::Args, compositor::Compositor, config::Config, keymap::Keymaps, ui::EditorView,
};
use helix_view::document::DocumentSavedEventResult;
use helix_view::{doc_mut, graphics::Rect, handlers::Handlers, theme, Editor};

use anyhow::Error;
use log::{debug, warn};
use tokio_stream::StreamExt;

pub struct Application {
    pub editor: Editor,
    pub compositor: Compositor,
    pub view: EditorView,
    pub jobs: Jobs,
}

#[derive(Debug)]
pub enum InputEvent {
    Key(helix_view::input::KeyEvent),
    ScrollLines {
        line_count: usize,
        direction: helix_core::movement::Direction,
    },
}

impl Application {
    fn emit_overlays(&mut self, cx: &mut gpui::ModelContext<'_, crate::Core>) {
        use crate::picker::Picker as PickerComponent;
        use crate::prompt::Prompt;
        use helix_term::ui::{overlay::Overlay, Picker};
        use std::path::PathBuf;

        let picker = if let Some(p) = self
            .compositor
            .find_id::<Overlay<Picker<PathBuf>>>(helix_term::ui::picker::ID)
        {
            println!("found file picker");
            Some(PickerComponent::make(&mut self.editor, &mut p.content))
        } else {
            None
        };
        let prompt = if let Some(p) = self.compositor.find::<helix_term::ui::Prompt>() {
            Some(Prompt::make(&mut self.editor, p))
        } else {
            None
        };

        if let Some(picker) = picker {
            cx.emit(crate::Update::Picker(picker));
        }

        if let Some(prompt) = prompt {
            cx.emit(crate::Update::Prompt(prompt));
        }

        if let Some(info) = self.editor.autoinfo.take() {
            cx.emit(crate::Update::Info(info));
        }
    }

    fn handle_input_event(
        &mut self,
        event: InputEvent,
        cx: &mut gpui::ModelContext<'_, crate::Core>,
    ) {
        use helix_term::compositor::{Component, EventResult};
        // println!("INPUT EVENT {:?}", event);

        let mut comp_ctx = helix_term::compositor::Context {
            editor: &mut self.editor,
            scroll: None,
            jobs: &mut self.jobs,
        };
        match event {
            InputEvent::Key(key) => {
                let mut is_handled = self
                    .compositor
                    .handle_event(&helix_view::input::Event::Key(key), &mut comp_ctx);
                if !is_handled {
                    let event = &helix_view::input::Event::Key(key);
                    let res = self.view.handle_event(event, &mut comp_ctx);
                    is_handled = matches!(res, EventResult::Consumed(_));
                    if let EventResult::Consumed(Some(cb)) = res {
                        cb(&mut self.compositor, &mut comp_ctx);
                    }
                }
                let _is_handled = is_handled;
                // println!("KEY IS HANDLED ? {:?}", is_handled);
                self.emit_overlays(cx);
                cx.emit(crate::Update::Redraw);
            }
            InputEvent::ScrollLines { .. } => {}
        }
    }

    pub fn handle_document_write(&mut self, doc_save_event: &DocumentSavedEventResult) {
        let doc_save_event = match doc_save_event {
            Ok(event) => event,
            Err(err) => {
                self.editor.set_error(err.to_string());
                return;
            }
        };

        let doc = match self.editor.document_mut(doc_save_event.doc_id) {
            None => {
                warn!(
                    "received document saved event for non-existent doc id: {}",
                    doc_save_event.doc_id
                );

                return;
            }
            Some(doc) => doc,
        };

        debug!(
            "document {:?} saved with revision {}",
            doc.path(),
            doc_save_event.revision
        );

        doc.set_last_saved_revision(doc_save_event.revision);

        let lines = doc_save_event.text.len_lines();
        let bytes = doc_save_event.text.len_bytes();

        self.editor
            .set_doc_path(doc_save_event.doc_id, &doc_save_event.path);
        // TODO: fix being overwritten by lsp
        self.editor.set_status(format!(
            "'{}' written, {}L {}B",
            get_relative_path(&doc_save_event.path).to_string_lossy(),
            lines,
            bytes
        ));
    }

    pub async fn step<S>(
        &mut self,
        input_stream: &mut S,
        cx: &mut gpui::ModelContext<'_, crate::Core>,
    ) where
        S: Stream<Item = InputEvent> + Unpin,
    {
        loop {
            tokio::select! {
                biased;

                Some(event) = input_stream.next() => {
                    self.handle_input_event(event, cx);
                    //self.handle_terminal_events(event).await;
                }
                Some(callback) = self.jobs.callbacks.recv() => {
                    self.jobs.handle_callback(&mut self.editor, &mut self.compositor, Ok(Some(callback)));
                    // self.render().await;
                }
                Some(msg) = self.jobs.status_messages.recv() => {
                    let severity = match msg.severity{
                        helix_event::status::Severity::Hint => Severity::Hint,
                        helix_event::status::Severity::Info => Severity::Info,
                        helix_event::status::Severity::Warning => Severity::Warning,
                        helix_event::status::Severity::Error => Severity::Error,
                    };
                    let status = crate::EditorStatus { status: msg.message.to_string(), severity };
                    cx.emit(crate::Update::EditorStatus(status));
                    // TODO: show multiple status messages at once to avoid clobbering
                    self.editor.status_msg = Some((msg.message, severity));
                    helix_event::request_redraw();
                }
                Some(callback) = self.jobs.wait_futures.next() => {
                    self.jobs.handle_callback(&mut self.editor, &mut self.compositor, callback);
                    // self.render().await;
                }
                event = self.editor.wait_event() => {
                    use helix_view::editor::EditorEvent;
                    // println!("editor event {:?}", event);
                    match event {
                        EditorEvent::DocumentSaved(event) => {
                            self.handle_document_write(&event);
                            cx.emit(crate::Update::EditorEvent(EditorEvent::DocumentSaved(event)));
                        }
                        EditorEvent::IdleTimer => {
                            self.editor.clear_idle_timer();
                            /* dont send */
                        }
                        EditorEvent::Redraw => {
                             cx.emit(crate::Update::EditorEvent(EditorEvent::Redraw));
                        }
                        EditorEvent::ConfigEvent(_) => {
                            /* TODO */
                        }
                        EditorEvent::LanguageServerMessage(_) => {
                            /* TODO */
                        }
                        EditorEvent::DebuggerEvent(_) => {
                            /* TODO */
                        }
                    }
                }
                else => break,
            }
        }
    }
}

pub fn init_editor(
    args: Args,
    config: Config,
    lang_loader: syntax::Loader,
) -> Result<Application, Error> {
    use helix_view::editor::Action;

    let mut theme_parent_dirs = vec![helix_loader::config_dir()];
    theme_parent_dirs.extend(helix_loader::runtime_dirs().iter().cloned());
    let theme_loader = std::sync::Arc::new(theme::Loader::new(&theme_parent_dirs));

    let true_color = true;
    let theme = config
        .theme
        .as_ref()
        .and_then(|theme| {
            theme_loader
                .load(theme)
                .map_err(|e| {
                    log::warn!("failed to load theme `{}` - {}", theme, e);
                    e
                })
                .ok()
                .filter(|theme| (true_color || theme.is_16_color()))
        })
        .unwrap_or_else(|| theme_loader.default_theme(true_color));

    let syn_loader = Arc::new(ArcSwap::from_pointee(lang_loader));
    let config = Arc::new(ArcSwap::from_pointee(config));

    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 25,
    };
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let (tx1, _rx1) = tokio::sync::mpsc::channel(1);
    let handlers = Handlers {
        completions: tx,
        signature_hints: tx1,
    };
    let mut editor = Editor::new(
        area,
        theme_loader.clone(),
        syn_loader.clone(),
        Arc::new(Map::new(Arc::clone(&config), |config: &Config| {
            &config.editor
        })),
        handlers,
    );

    if args.load_tutor {
        let path = helix_loader::runtime_file(Path::new("tutor"));
        // let path = Path::new("./test.rs");
        let doc_id = editor.open(&path, Action::VerticalSplit)?;
        let view_id = editor.tree.focus;
        let doc = doc_mut!(editor, &doc_id);
        let pos = Selection::point(pos_at_coords(
            doc.text().slice(..),
            Position::new(0, 0),
            true,
        ));
        doc.set_selection(view_id, pos);

        // Unset path to prevent accidentally saving to the original tutor file.
        doc_mut!(editor).set_path(None);
    } else {
        editor.new_file(Action::VerticalSplit);
    }

    editor.set_theme(theme);

    let keys = Box::new(Map::new(Arc::clone(&config), |config: &Config| {
        &config.keys
    }));
    let compositor = Compositor::new(Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 25,
    });
    let keymaps = Keymaps::new(keys);
    let view = EditorView::new(keymaps);
    let jobs = Jobs::new();

    helix_term::events::register();

    Ok(Application {
        editor,
        compositor,
        view,
        jobs,
    })
}
