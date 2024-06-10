use std::{collections::btree_map::Entry, path::Path, sync::Arc};

use arc_swap::{access::Map, ArcSwap};
use futures_util::FutureExt;
use helix_core::diagnostic::Severity;
use helix_core::{pos_at_coords, syntax, Position, Selection};

use helix_lsp::{
    lsp::{self, notification::Notification},
    LanguageServerId, LspProgressMap,
};
use helix_stdx::path::get_relative_path;
use helix_term::job::Jobs;
use helix_term::{
    args::Args, compositor::Compositor, config::Config, keymap::Keymaps, ui::EditorView,
};
use helix_view::document::DocumentSavedEventResult;
use helix_view::{doc_mut, graphics::Rect, handlers::Handlers, theme, Editor};

use anyhow::Error;
use log::{debug, error, info, warn};
use serde_json::json;
use tokio_stream::StreamExt;

pub struct Application {
    pub editor: Editor,
    pub compositor: Compositor,
    pub view: EditorView,
    pub jobs: Jobs,
    pub lsp_progress: LspProgressMap,
}

#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(helix_view::input::KeyEvent),
    ScrollLines {
        line_count: usize,
        direction: helix_core::movement::Direction,
        view_id: helix_view::ViewId,
    },
}

pub struct Input;

impl gpui::EventEmitter<InputEvent> for Input {}

pub struct Crank;

impl gpui::EventEmitter<()> for Crank {}

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

    pub fn handle_input_event(
        &mut self,
        event: InputEvent,
        cx: &mut gpui::ModelContext<'_, crate::Core>,
        handle: tokio::runtime::Handle,
    ) {
        let _guard = handle.enter();
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
            InputEvent::ScrollLines {
                line_count,
                direction,
                ..
            } => {
                let mut ctx = helix_term::commands::Context {
                    editor: &mut self.editor,
                    register: None,
                    count: None,
                    callback: Vec::new(),
                    on_next_key_callback: None,
                    jobs: &mut self.jobs,
                };
                helix_term::commands::scroll(&mut ctx, line_count, direction, false);
                cx.emit(crate::Update::Redraw);
            }
        }
    }

    fn handle_document_write(&mut self, doc_save_event: &DocumentSavedEventResult) {
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

    pub fn handle_crank_event(
        &mut self,
        _event: (),
        cx: &mut gpui::ModelContext<'_, crate::Core>,
        handle: tokio::runtime::Handle,
    ) {
        let _guard = handle.enter();

        self.step(cx).now_or_never();
        /*
        use std::future::Future;
        let fut = self.step(cx);
        let mut fut = Box::pin(fut);
        handle.block_on(std::future::poll_fn(move |cx| {
            let _ = fut.as_mut().poll(cx);
            Poll::Ready(())
        }));
        */
    }

    pub async fn step(&mut self, cx: &mut gpui::ModelContext<'_, crate::Core>) {
        loop {
            tokio::select! {
                biased;

                // Some(event) = input_stream.next() => {
                //     // self.handle_input_event(event, cx);
                //     //self.handle_terminal_events(event).await;
                // }
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
                        EditorEvent::LanguageServerMessage((id, call)) => {
                            self.handle_language_server_message(call, id).await;
                        }
                        EditorEvent::DebuggerEvent(_) => {
                            /* TODO */
                        }
                    }
                }
                else => {
                    break;
                }
            }
        }
    }

    // copy pasted from helix_term/src/application.rs
    async fn handle_language_server_message(
        &mut self,
        call: helix_lsp::Call,
        server_id: LanguageServerId,
    ) {
        use helix_lsp::{Call, MethodCall, Notification};

        macro_rules! language_server {
            () => {
                match self.editor.language_server_by_id(server_id) {
                    Some(language_server) => language_server,
                    None => {
                        warn!("can't find language server with id `{}`", server_id);
                        return;
                    }
                }
            };
        }

        match call {
            Call::Notification(helix_lsp::jsonrpc::Notification { method, params, .. }) => {
                let notification = match Notification::parse(&method, params) {
                    Ok(notification) => notification,
                    Err(helix_lsp::Error::Unhandled) => {
                        info!("Ignoring Unhandled notification from Language Server");
                        return;
                    }
                    Err(err) => {
                        error!(
                            "Ignoring unknown notification from Language Server: {}",
                            err
                        );
                        return;
                    }
                };

                match notification {
                    Notification::Initialized => {
                        let language_server = language_server!();

                        // Trigger a workspace/didChangeConfiguration notification after initialization.
                        // This might not be required by the spec but Neovim does this as well, so it's
                        // probably a good idea for compatibility.
                        if let Some(config) = language_server.config() {
                            tokio::spawn(language_server.did_change_configuration(config.clone()));
                        }

                        let docs = self
                            .editor
                            .documents()
                            .filter(|doc| doc.supports_language_server(server_id));

                        // trigger textDocument/didOpen for docs that are already open
                        for doc in docs {
                            let url = match doc.url() {
                                Some(url) => url,
                                None => continue, // skip documents with no path
                            };

                            let language_id =
                                doc.language_id().map(ToOwned::to_owned).unwrap_or_default();

                            tokio::spawn(language_server.text_document_did_open(
                                url,
                                doc.version(),
                                doc.text(),
                                language_id,
                            ));
                        }
                    }
                    Notification::PublishDiagnostics(mut params) => {
                        let path = match params.uri.to_file_path() {
                            Ok(path) => helix_stdx::path::normalize(path),
                            Err(_) => {
                                log::error!("Unsupported file URI: {}", params.uri);
                                return;
                            }
                        };
                        let language_server = language_server!();
                        if !language_server.is_initialized() {
                            log::error!("Discarding publishDiagnostic notification sent by an uninitialized server: {}", language_server.name());
                            return;
                        }
                        // have to inline the function because of borrow checking...
                        let doc = self.editor.documents.values_mut()
                            .find(|doc| doc.path().map(|p| p == &path).unwrap_or(false))
                            .filter(|doc| {
                                if let Some(version) = params.version {
                                    if version != doc.version() {
                                        log::info!("Version ({version}) is out of date for {path:?} (expected ({}), dropping PublishDiagnostic notification", doc.version());
                                        return false;
                                    }
                                }
                                true
                            });

                        let mut unchanged_diag_sources = Vec::new();
                        if let Some(doc) = &doc {
                            let lang_conf = doc.language.clone();

                            if let Some(lang_conf) = &lang_conf {
                                if let Some(old_diagnostics) = self.editor.diagnostics.get(&path) {
                                    if !lang_conf.persistent_diagnostic_sources.is_empty() {
                                        // Sort diagnostics first by severity and then by line numbers.
                                        // Note: The `lsp::DiagnosticSeverity` enum is already defined in decreasing order
                                        params
                                            .diagnostics
                                            .sort_unstable_by_key(|d| (d.severity, d.range.start));
                                    }
                                    for source in &lang_conf.persistent_diagnostic_sources {
                                        let new_diagnostics = params
                                            .diagnostics
                                            .iter()
                                            .filter(|d| d.source.as_ref() == Some(source));
                                        let old_diagnostics = old_diagnostics
                                            .iter()
                                            .filter(|(d, d_server)| {
                                                *d_server == server_id
                                                    && d.source.as_ref() == Some(source)
                                            })
                                            .map(|(d, _)| d);
                                        if new_diagnostics.eq(old_diagnostics) {
                                            unchanged_diag_sources.push(source.clone())
                                        }
                                    }
                                }
                            }
                        }

                        let diagnostics = params.diagnostics.into_iter().map(|d| (d, server_id));

                        // Insert the original lsp::Diagnostics here because we may have no open document
                        // for diagnosic message and so we can't calculate the exact position.
                        // When using them later in the diagnostics picker, we calculate them on-demand.
                        let diagnostics = match self.editor.diagnostics.entry(path) {
                            Entry::Occupied(o) => {
                                let current_diagnostics = o.into_mut();
                                // there may entries of other language servers, which is why we can't overwrite the whole entry
                                current_diagnostics.retain(|(_, lsp_id)| *lsp_id != server_id);
                                current_diagnostics.extend(diagnostics);
                                current_diagnostics
                                // Sort diagnostics first by severity and then by line numbers.
                            }
                            Entry::Vacant(v) => v.insert(diagnostics.collect()),
                        };

                        // Sort diagnostics first by severity and then by line numbers.
                        // Note: The `lsp::DiagnosticSeverity` enum is already defined in decreasing order
                        diagnostics.sort_unstable_by_key(|(d, server_id)| {
                            (d.severity, d.range.start, *server_id)
                        });

                        if let Some(doc) = doc {
                            let diagnostic_of_language_server_and_not_in_unchanged_sources =
                                |diagnostic: &lsp::Diagnostic, ls_id| {
                                    ls_id == server_id
                                        && diagnostic.source.as_ref().map_or(true, |source| {
                                            !unchanged_diag_sources.contains(source)
                                        })
                                };
                            let diagnostics = Editor::doc_diagnostics_with_filter(
                                &self.editor.language_servers,
                                &self.editor.diagnostics,
                                doc,
                                diagnostic_of_language_server_and_not_in_unchanged_sources,
                            );
                            doc.replace_diagnostics(
                                diagnostics,
                                &unchanged_diag_sources,
                                Some(server_id),
                            );
                        }
                    }
                    Notification::ShowMessage(params) => {
                        log::warn!("unhandled window/showMessage: {:?}", params);
                    }
                    Notification::LogMessage(params) => {
                        log::info!("window/logMessage: {:?}", params);
                    }
                    Notification::ProgressMessage(_params) => {
                        //     if !self
                        //         .compositor
                        //         .has_component(std::any::type_name::<ui::Prompt>()) =>
                        // {
                        // let editor_view = self
                        //     .compositor
                        //     .find::<ui::EditorView>()
                        //     .expect("expected at least one EditorView");
                        // let lsp::ProgressParams { token, value } = params;

                        // let lsp::ProgressParamsValue::WorkDone(work) = value;
                        // let parts = match &work {
                        //     lsp::WorkDoneProgress::Begin(lsp::WorkDoneProgressBegin {
                        //         title,
                        //         message,
                        //         percentage,
                        //         ..
                        //     }) => (Some(title), message, percentage),
                        //     lsp::WorkDoneProgress::Report(lsp::WorkDoneProgressReport {
                        //         message,
                        //         percentage,
                        //         ..
                        //     }) => (None, message, percentage),
                        //     lsp::WorkDoneProgress::End(lsp::WorkDoneProgressEnd { message }) => {
                        //         if message.is_some() {
                        //             (None, message, &None)
                        //         } else {
                        //             self.lsp_progress.end_progress(server_id, &token);
                        //             if !self.lsp_progress.is_progressing(server_id) {
                        //                 editor_view.spinners_mut().get_or_create(server_id).stop();
                        //             }
                        //             self.editor.clear_status();

                        //             // we want to render to clear any leftover spinners or messages
                        //             return;
                        //         }
                        //     }
                        // };

                        // let token_d: &dyn std::fmt::Display = match &token {
                        //     lsp::NumberOrString::Number(n) => n,
                        //     lsp::NumberOrString::String(s) => s,
                        // };

                        // let status = match parts {
                        //     (Some(title), Some(message), Some(percentage)) => {
                        //         format!("[{}] {}% {} - {}", token_d, percentage, title, message)
                        //     }
                        //     (Some(title), None, Some(percentage)) => {
                        //         format!("[{}] {}% {}", token_d, percentage, title)
                        //     }
                        //     (Some(title), Some(message), None) => {
                        //         format!("[{}] {} - {}", token_d, title, message)
                        //     }
                        //     (None, Some(message), Some(percentage)) => {
                        //         format!("[{}] {}% {}", token_d, percentage, message)
                        //     }
                        //     (Some(title), None, None) => {
                        //         format!("[{}] {}", token_d, title)
                        //     }
                        //     (None, Some(message), None) => {
                        //         format!("[{}] {}", token_d, message)
                        //     }
                        //     (None, None, Some(percentage)) => {
                        //         format!("[{}] {}%", token_d, percentage)
                        //     }
                        //     (None, None, None) => format!("[{}]", token_d),
                        // };

                        // if let lsp::WorkDoneProgress::End(_) = work {
                        //     self.lsp_progress.end_progress(server_id, &token);
                        //     if !self.lsp_progress.is_progressing(server_id) {
                        //         editor_view.spinners_mut().get_or_create(server_id).stop();
                        //     }
                        // } else {
                        //     self.lsp_progress.update(server_id, token, work);
                        // }

                        // if self.config.load().editor.lsp.display_messages {
                        //     self.editor.set_status(status);
                        // }
                    }
                    Notification::ProgressMessage(_params) => {
                        // do nothing
                    }
                    Notification::Exit => {
                        self.editor.set_status("Language server exited");

                        // LSPs may produce diagnostics for files that haven't been opened in helix,
                        // we need to clear those and remove the entries from the list if this leads to
                        // an empty diagnostic list for said files
                        for diags in self.editor.diagnostics.values_mut() {
                            diags.retain(|(_, lsp_id)| *lsp_id != server_id);
                        }

                        self.editor.diagnostics.retain(|_, diags| !diags.is_empty());

                        // Clear any diagnostics for documents with this server open.
                        for doc in self.editor.documents_mut() {
                            doc.clear_diagnostics(Some(server_id));
                        }

                        // Remove the language server from the registry.
                        self.editor.language_servers.remove_by_id(server_id);
                    }
                }
            }
            Call::MethodCall(helix_lsp::jsonrpc::MethodCall {
                method, params, id, ..
            }) => {
                let reply = match MethodCall::parse(&method, params) {
                    Err(helix_lsp::Error::Unhandled) => {
                        error!(
                            "Language Server: Method {} not found in request {}",
                            method, id
                        );
                        Err(helix_lsp::jsonrpc::Error {
                            code: helix_lsp::jsonrpc::ErrorCode::MethodNotFound,
                            message: format!("Method not found: {}", method),
                            data: None,
                        })
                    }
                    Err(err) => {
                        log::error!(
                            "Language Server: Received malformed method call {} in request {}: {}",
                            method,
                            id,
                            err
                        );
                        Err(helix_lsp::jsonrpc::Error {
                            code: helix_lsp::jsonrpc::ErrorCode::ParseError,
                            message: format!("Malformed method call: {}", method),
                            data: None,
                        })
                    }
                    Ok(MethodCall::WorkDoneProgressCreate(params)) => {
                        self.lsp_progress.create(server_id, params.token);

                        // let editor_view = self
                        //     .compositor
                        //     .find::<ui::EditorView>()
                        //     .expect("expected at least one EditorView");
                        // let spinner = editor_view.spinners_mut().get_or_create(server_id);
                        // if spinner.is_stopped() {
                        //     spinner.start();
                        // }

                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::ApplyWorkspaceEdit(params)) => {
                        let language_server = language_server!();
                        if language_server.is_initialized() {
                            let offset_encoding = language_server.offset_encoding();
                            let res = self
                                .editor
                                .apply_workspace_edit(offset_encoding, &params.edit);

                            Ok(json!(lsp::ApplyWorkspaceEditResponse {
                                applied: res.is_ok(),
                                failure_reason: res.as_ref().err().map(|err| err.kind.to_string()),
                                failed_change: res
                                    .as_ref()
                                    .err()
                                    .map(|err| err.failed_change_idx as u32),
                            }))
                        } else {
                            Err(helix_lsp::jsonrpc::Error {
                                code: helix_lsp::jsonrpc::ErrorCode::InvalidRequest,
                                message: "Server must be initialized to request workspace edits"
                                    .to_string(),
                                data: None,
                            })
                        }
                    }
                    Ok(MethodCall::WorkspaceFolders) => {
                        Ok(json!(&*language_server!().workspace_folders().await))
                    }
                    Ok(MethodCall::WorkspaceConfiguration(params)) => {
                        let language_server = language_server!();
                        let result: Vec<_> = params
                            .items
                            .iter()
                            .map(|item| {
                                let mut config = language_server.config()?;
                                if let Some(section) = item.section.as_ref() {
                                    // for some reason some lsps send an empty string (observed in 'vscode-eslint-language-server')
                                    if !section.is_empty() {
                                        for part in section.split('.') {
                                            config = config.get(part)?;
                                        }
                                    }
                                }
                                Some(config)
                            })
                            .collect();
                        Ok(json!(result))
                    }
                    Ok(MethodCall::RegisterCapability(params)) => {
                        if let Some(client) = self.editor.language_servers.get_by_id(server_id) {
                            for reg in params.registrations {
                                match reg.method.as_str() {
                                    lsp::notification::DidChangeWatchedFiles::METHOD => {
                                        let Some(options) = reg.register_options else {
                                            continue;
                                        };
                                        let ops: lsp::DidChangeWatchedFilesRegistrationOptions =
                                            match serde_json::from_value(options) {
                                                Ok(ops) => ops,
                                                Err(err) => {
                                                    log::warn!("Failed to deserialize DidChangeWatchedFilesRegistrationOptions: {err}");
                                                    continue;
                                                }
                                            };
                                        self.editor.language_servers.file_event_handler.register(
                                            client.id(),
                                            Arc::downgrade(client),
                                            reg.id,
                                            ops,
                                        )
                                    }
                                    _ => {
                                        // Language Servers based on the `vscode-languageserver-node` library often send
                                        // client/registerCapability even though we do not enable dynamic registration
                                        // for most capabilities. We should send a MethodNotFound JSONRPC error in this
                                        // case but that rejects the registration promise in the server which causes an
                                        // exit. So we work around this by ignoring the request and sending back an OK
                                        // response.
                                        log::warn!("Ignoring a client/registerCapability request because dynamic capability registration is not enabled. Please report this upstream to the language server");
                                    }
                                }
                            }
                        }

                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::UnregisterCapability(params)) => {
                        for unreg in params.unregisterations {
                            match unreg.method.as_str() {
                                lsp::notification::DidChangeWatchedFiles::METHOD => {
                                    self.editor
                                        .language_servers
                                        .file_event_handler
                                        .unregister(server_id, unreg.id);
                                }
                                _ => {
                                    log::warn!("Received unregistration request for unsupported method: {}", unreg.method);
                                }
                            }
                        }
                        Ok(serde_json::Value::Null)
                    }
                    Ok(MethodCall::ShowDocument(_params)) => {
                        // let language_server = language_server!();
                        // let offset_encoding = language_server.offset_encoding();

                        // let result = self.handle_show_document(params, offset_encoding);
                        let result = lsp::ShowDocumentResult { success: true };
                        Ok(json!(result))
                    }
                };

                tokio::spawn(language_server!().reply(id, reply));
            }
            Call::Invalid { id } => log::error!("LSP invalid method call id={:?}", id),
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
        lsp_progress: LspProgressMap::new(),
    })
}
