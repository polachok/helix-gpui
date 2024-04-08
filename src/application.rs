use arc_swap::{access::Map, ArcSwap};
use std::{collections::btree_map::Entry, io::stdin, path::Path, sync::Arc};

use helix_core::{diagnostic::Severity, pos_at_coords, syntax, Position, Selection};

use crate::compositor::{Compositor, Context};
use helix_stdx::path::get_relative_path;
use helix_term::{
    args::Args,
    config::Config,
    //handlers,
    job::Jobs,
    keymap::Keymaps,
    ui::{self, overlay::overlaid},
};
use helix_view::{
    align_view, doc_mut,
    document::DocumentSavedEventResult,
    editor::{ConfigEvent, EditorEvent},
    graphics::Rect,
    handlers::Handlers,
    theme,
    tree::Layout,
    Align, Editor,
};

use anyhow::{Context as _, Error};

pub struct Application {
    pub editor: Editor,
    pub keymaps: Keymaps,
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

    let true_color = config.editor.true_color || true;
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
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let (tx1, rx1) = tokio::sync::mpsc::channel(1);
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
        //let path = helix_loader::runtime_file(Path::new("tutor"));
        let path = Path::new("./test.rs");
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
        //doc_mut!(editor).set_path(None);
    }

    //editor.new_file(Action::VerticalSplit);
    editor.set_theme(theme);

    let compositor = Compositor::new(area);
    let keys = Box::new(Map::new(Arc::clone(&config), |config: &Config| {
        &config.keys
    }));
    let keymaps = Keymaps::new(keys);

    Ok(Application { editor, keymaps })
}
