use crate::utils::color_to_hsla;
use crate::Core;
use gpui::*;
use helix_view::{DocumentId, ViewId};

#[derive(IntoElement)]
pub struct StatusLine {
    core: Model<Core>,
    doc_id: DocumentId,
    view_id: ViewId,
    focused: bool,
    style: TextStyle,
}

impl StatusLine {
    pub fn new(
        core: Model<Core>,
        doc_id: DocumentId,
        view_id: ViewId,
        focused: bool,
        style: TextStyle,
    ) -> Self {
        Self {
            core,
            doc_id,
            view_id,
            focused,
            style,
        }
    }

    fn style(&self, cx: &mut WindowContext<'_>) -> (Hsla, Hsla) {
        let editor = &self.core.read(cx).lock().unwrap().editor;
        let base_style = if self.focused {
            editor.theme.get("ui.statusline")
        } else {
            editor.theme.get("ui.statusline.inactive")
        };
        let base_fg = base_style
            .fg
            .and_then(color_to_hsla)
            .unwrap_or(hsla(0.5, 0.5, 0.5, 1.));
        let base_bg = base_style
            .bg
            .and_then(color_to_hsla)
            .unwrap_or(hsla(0.5, 0.5, 0.5, 1.));
        (base_fg, base_bg)
    }

    fn text(
        &self,
        cx: &mut WindowContext<'_>,
        base_fg: Hsla,
        base_bg: Hsla,
    ) -> (StyledText, StyledText, StyledText) {
        use self::copy_pasta::{render_status_parts, RenderContext};
        let editor = &self.core.read(cx).lock().unwrap().editor;
        let doc = editor.document(self.doc_id).unwrap();
        let view = editor.tree.get(self.view_id);

        let mut ctx = RenderContext {
            editor: &editor,
            doc,
            view,
            focused: self.focused,
        };

        let parts = render_status_parts(&mut ctx);

        let styled = |spans: Vec<tui::text::Span<'_>>| {
            let mut text = String::new();
            let mut runs = Vec::new();
            let mut idx = 0;
            for span in spans {
                let len = span.content.len();
                text.push_str(&span.content);
                let fg = span.style.fg.and_then(color_to_hsla).unwrap_or(base_fg);
                let bg = span.style.bg.and_then(color_to_hsla).unwrap_or(base_bg);
                let mut run = HighlightStyle::default();
                run.color = Some(fg);
                run.background_color = Some(bg);
                runs.push(((idx..idx + len), run));
                idx += len;
            }
            StyledText::new(text).with_highlights(&self.style, runs)
        };

        (
            styled(parts.left),
            styled(parts.center),
            styled(parts.right),
        )
    }
}

impl RenderOnce for StatusLine {
    fn render(self, cx: &mut WindowContext<'_>) -> impl IntoElement {
        let (base_fg, base_bg) = self.style(cx);
        let parts = self.text(cx, base_fg, base_bg);
        let (left, center, right) = parts;

        div()
            .w_full()
            .flex()
            .flex_row()
            .bg(base_bg)
            .justify_between()
            .content_stretch()
            .text_size(self.style.font_size)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .content_stretch()
                    .child(left),
            )
            .child(div().flex().child(center))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_end()
                    .content_stretch()
                    .justify_end()
                    .child(right),
            )
        // )
    }
}

// copy/paste from helix term (ui/statusline.rs) going further
mod copy_pasta {
    use helix_core::{coords_at_pos, encoding, Position};
    use helix_view::document::DEFAULT_LANGUAGE_NAME;
    use helix_view::document::{Mode, SCRATCH_BUFFER_NAME};
    use helix_view::{Document, Editor, View};

    use helix_lsp::lsp::DiagnosticSeverity;
    use helix_view::editor::StatusLineElement as StatusLineElementID;

    use tui::text::{Span, Spans};

    pub struct RenderContext<'a> {
        pub editor: &'a Editor,
        pub doc: &'a Document,
        pub view: &'a View,
        pub focused: bool,
    }

    #[derive(Debug)]
    pub struct StatusLineElements<'a> {
        pub left: Vec<Span<'a>>,
        pub center: Vec<Span<'a>>,
        pub right: Vec<Span<'a>>,
    }

    pub fn render_status_parts<'a>(context: &mut RenderContext) -> StatusLineElements<'a> {
        let config = context.editor.config();

        let element_ids = &config.statusline.left;
        let left = element_ids
            .iter()
            .map(|element_id| get_render_function(*element_id))
            .flat_map(|render| render(context).0)
            .collect::<Vec<Span>>();

        let element_ids = &config.statusline.center;
        let center = element_ids
            .iter()
            .map(|element_id| get_render_function(*element_id))
            .flat_map(|render| render(context).0)
            .collect::<Vec<Span>>();

        let element_ids = &config.statusline.right;
        let right = element_ids
            .iter()
            .map(|element_id| get_render_function(*element_id))
            .flat_map(|render| render(context).0)
            .collect::<Vec<Span>>();
        StatusLineElements {
            left,
            right,
            center,
        }
    }

    fn get_render_function<'a>(
        element_id: StatusLineElementID,
    ) -> impl Fn(&RenderContext) -> Spans<'a> {
        match element_id {
            helix_view::editor::StatusLineElement::Mode => render_mode,
            helix_view::editor::StatusLineElement::Spinner => render_lsp_spinner,
            helix_view::editor::StatusLineElement::FileBaseName => render_file_base_name,
            helix_view::editor::StatusLineElement::FileName => render_file_name,
            helix_view::editor::StatusLineElement::FileAbsolutePath => render_file_absolute_path,
            helix_view::editor::StatusLineElement::FileModificationIndicator => {
                render_file_modification_indicator
            }
            helix_view::editor::StatusLineElement::ReadOnlyIndicator => render_read_only_indicator,
            helix_view::editor::StatusLineElement::FileEncoding => render_file_encoding,
            helix_view::editor::StatusLineElement::FileLineEnding => render_file_line_ending,
            helix_view::editor::StatusLineElement::FileType => render_file_type,
            helix_view::editor::StatusLineElement::Diagnostics => render_diagnostics,
            helix_view::editor::StatusLineElement::WorkspaceDiagnostics => {
                render_workspace_diagnostics
            }
            helix_view::editor::StatusLineElement::Selections => render_selections,
            helix_view::editor::StatusLineElement::PrimarySelectionLength => {
                render_primary_selection_length
            }
            helix_view::editor::StatusLineElement::Position => render_position,
            helix_view::editor::StatusLineElement::PositionPercentage => render_position_percentage,
            helix_view::editor::StatusLineElement::TotalLineNumbers => render_total_line_numbers,
            helix_view::editor::StatusLineElement::Separator => render_separator,
            helix_view::editor::StatusLineElement::Spacer => render_spacer,
            helix_view::editor::StatusLineElement::VersionControl => render_version_control,
            helix_view::editor::StatusLineElement::Register => render_register,
        }
    }

    fn render_mode<'a>(context: &RenderContext) -> Spans<'a> {
        let visible = context.focused;
        let config = context.editor.config();
        let modenames = &config.statusline.mode;
        let modename = if visible {
            match context.editor.mode() {
                Mode::Insert => modenames.insert.clone(),
                Mode::Select => modenames.select.clone(),
                Mode::Normal => modenames.normal.clone(),
            }
        } else {
            // If not focused, explicitly leave an empty space.
            " ".into()
        };
        let modename = format!(" {} ", modename);
        if visible && config.color_modes {
            Span::styled(
                modename,
                match context.editor.mode() {
                    Mode::Insert => context.editor.theme.get("ui.statusline.insert"),
                    Mode::Select => context.editor.theme.get("ui.statusline.select"),
                    Mode::Normal => context.editor.theme.get("ui.statusline.normal"),
                },
            )
            .into()
        } else {
            Span::raw(modename).into()
        }
    }

    // TODO think about handling multiple language servers
    fn render_lsp_spinner<'a>(context: &RenderContext) -> Spans<'a> {
        let _language_server = context.doc.language_servers().next();
        Span::raw(
            "".to_string(), // language_server
                            //     .and_then(|srv| {
                            //         context
                            //             .spinners
                            //             .get(srv.id())
                            //             .and_then(|spinner| spinner.frame())
                            //     })
                            //     // Even if there's no spinner; reserve its space to avoid elements frequently shifting.
                            //     .unwrap_or(" ")
                            //     .to_string(),
        )
        .into()
    }

    fn render_diagnostics<'a>(context: &RenderContext) -> Spans<'a> {
        let (warnings, errors) =
            context
                .doc
                .diagnostics()
                .iter()
                .fold((0, 0), |mut counts, diag| {
                    use helix_core::diagnostic::Severity;
                    match diag.severity {
                        Some(Severity::Warning) => counts.0 += 1,
                        Some(Severity::Error) | None => counts.1 += 1,
                        _ => {}
                    }
                    counts
                });

        let mut output = Spans::default();

        if warnings > 0 {
            output.0.push(Span::styled(
                "●".to_string(),
                context.editor.theme.get("warning"),
            ));
            output.0.push(Span::raw(format!(" {} ", warnings)));
        }

        if errors > 0 {
            output.0.push(Span::styled(
                "●".to_string(),
                context.editor.theme.get("error"),
            ));
            output.0.push(Span::raw(format!(" {} ", errors)));
        }

        output
    }

    fn render_workspace_diagnostics<'a>(context: &RenderContext) -> Spans<'a> {
        let (warnings, errors) =
            context
                .editor
                .diagnostics
                .values()
                .flatten()
                .fold((0, 0), |mut counts, (diag, _)| {
                    match diag.severity {
                        Some(DiagnosticSeverity::WARNING) => counts.0 += 1,
                        Some(DiagnosticSeverity::ERROR) | None => counts.1 += 1,
                        _ => {}
                    }
                    counts
                });

        let mut output = Spans::default();

        if warnings > 0 || errors > 0 {
            output.0.push(Span::raw(" W "));
        }

        if warnings > 0 {
            output.0.push(Span::styled(
                "●".to_string(),
                context.editor.theme.get("warning"),
            ));
            output.0.push(Span::raw(format!(" {} ", warnings)));
        }

        if errors > 0 {
            output.0.push(Span::styled(
                "●".to_string(),
                context.editor.theme.get("error"),
            ));
            output.0.push(Span::raw(format!(" {} ", errors)));
        }

        output
    }

    fn render_selections<'a>(context: &RenderContext) -> Spans<'a> {
        let count = context.doc.selection(context.view.id).len();
        Span::raw(format!(
            " {} sel{} ",
            count,
            if count == 1 { "" } else { "s" }
        ))
        .into()
    }

    fn render_primary_selection_length<'a>(context: &RenderContext) -> Spans<'a> {
        let tot_sel = context.doc.selection(context.view.id).primary().len();
        Span::raw(format!(
            " {} char{} ",
            tot_sel,
            if tot_sel == 1 { "" } else { "s" }
        ))
        .into()
    }

    fn get_position(context: &RenderContext) -> Position {
        coords_at_pos(
            context.doc.text().slice(..),
            context
                .doc
                .selection(context.view.id)
                .primary()
                .cursor(context.doc.text().slice(..)),
        )
    }

    fn render_position<'a>(context: &RenderContext) -> Spans<'a> {
        let position = get_position(context);
        Span::raw(format!(" {}:{} ", position.row + 1, position.col + 1)).into()
    }

    fn render_total_line_numbers<'a>(context: &RenderContext) -> Spans<'a> {
        let total_line_numbers = context.doc.text().len_lines();
        Span::raw(format!(" {} ", total_line_numbers)).into()
    }

    fn render_position_percentage<'a>(context: &RenderContext) -> Spans<'a> {
        let position = get_position(context);
        let maxrows = context.doc.text().len_lines();
        Span::raw(format!("{}%", (position.row + 1) * 100 / maxrows)).into()
    }

    fn render_file_encoding<'a>(context: &RenderContext) -> Spans<'a> {
        let enc = context.doc.encoding();

        if enc != encoding::UTF_8 {
            Span::raw(format!(" {} ", enc.name())).into()
        } else {
            Spans::default()
        }
    }

    fn render_file_line_ending<'a>(context: &RenderContext) -> Spans<'a> {
        use helix_core::LineEnding::*;
        let line_ending = match context.doc.line_ending {
            Crlf => "CRLF",
            LF => "LF",
            #[cfg(feature = "unicode-lines")]
            VT => "VT", // U+000B -- VerticalTab
            #[cfg(feature = "unicode-lines")]
            FF => "FF", // U+000C -- FormFeed
            #[cfg(feature = "unicode-lines")]
            CR => "CR", // U+000D -- CarriageReturn
            #[cfg(feature = "unicode-lines")]
            Nel => "NEL", // U+0085 -- NextLine
            #[cfg(feature = "unicode-lines")]
            LS => "LS", // U+2028 -- Line Separator
            #[cfg(feature = "unicode-lines")]
            PS => "PS", // U+2029 -- ParagraphSeparator
        };

        Span::raw(format!(" {} ", line_ending)).into()
    }

    fn render_file_type<'a>(context: &RenderContext) -> Spans<'a> {
        let file_type = context.doc.language_name().unwrap_or(DEFAULT_LANGUAGE_NAME);

        Span::raw(format!(" {} ", file_type)).into()
    }

    fn render_file_name<'a>(context: &RenderContext) -> Spans<'a> {
        let title = {
            let rel_path = context.doc.relative_path();
            let path = rel_path
                .as_ref()
                .map(|p| p.to_string_lossy())
                .unwrap_or_else(|| SCRATCH_BUFFER_NAME.into());
            format!(" {} ", path)
        };

        Span::raw(title).into()
    }

    fn render_file_absolute_path<'a>(context: &RenderContext) -> Spans<'a> {
        let title = {
            let path = context.doc.path();
            let path = path
                .as_ref()
                .map(|p| p.to_string_lossy())
                .unwrap_or_else(|| SCRATCH_BUFFER_NAME.into());
            format!(" {} ", path)
        };

        Span::raw(title).into()
    }

    fn render_file_modification_indicator<'a>(context: &RenderContext) -> Spans<'a> {
        let title = (if context.doc.is_modified() {
            "[+]"
        } else {
            "   "
        })
        .to_string();

        Span::raw(title).into()
    }

    fn render_read_only_indicator<'a>(context: &RenderContext) -> Spans<'a> {
        let title = if context.doc.readonly {
            " [readonly] "
        } else {
            ""
        }
        .to_string();
        Span::raw(title).into()
    }

    fn render_file_base_name<'a>(context: &RenderContext) -> Spans<'a> {
        let title = {
            let rel_path = context.doc.relative_path();
            let path = rel_path
                .as_ref()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy()))
                .unwrap_or_else(|| SCRATCH_BUFFER_NAME.into());
            format!(" {} ", path)
        };

        Span::raw(title).into()
    }

    fn render_separator<'a>(context: &RenderContext) -> Spans<'a> {
        let sep = &context.editor.config().statusline.separator;

        Span::styled(
            sep.to_string(),
            context.editor.theme.get("ui.statusline.separator"),
        )
        .into()
    }

    fn render_spacer<'a>(_context: &RenderContext) -> Spans<'a> {
        Span::raw(" ").into()
    }

    fn render_version_control<'a>(context: &RenderContext) -> Spans<'a> {
        let head = context
            .doc
            .version_control_head()
            .unwrap_or_default()
            .to_string();

        Span::raw(head).into()
    }

    fn render_register<'a>(context: &RenderContext) -> Spans<'a> {
        if let Some(reg) = context.editor.selected_register {
            Span::raw(format!(" reg={} ", reg)).into()
        } else {
            Spans::default()
        }
    }
}
