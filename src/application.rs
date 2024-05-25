use arc_swap::{access::Map, ArcSwap};
use helix_core::{pos_at_coords, syntax, Position, Selection};
use std::{path::Path, sync::Arc};

use helix_term::{
    args::Args, compositor::Compositor, config::Config, keymap::Keymaps, ui::EditorView,
};
use helix_view::{doc_mut, graphics::Rect, handlers::Handlers, theme, Editor};

use anyhow::Error;

pub struct Application {
    pub editor: Editor,
    pub compositor: Compositor,
    pub view: EditorView,
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

    helix_term::events::register();

    Ok(Application {
        editor,
        compositor,
        view,
    })
}
