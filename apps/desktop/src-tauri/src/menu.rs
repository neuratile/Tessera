//! Native menu bar.
//!
//! Builds the application menu (File / Edit / View / AI / Help) at
//! startup and routes user clicks to the renderer through a single
//! `app:menu` Tauri event. The renderer's `useAppMenuEvents` hook
//! listens to that event and dispatches into the relevant Zustand
//! store.
//!
//! Why a single event with an id payload rather than one event per
//! command? It keeps the Rust↔TS surface tiny (one schema entry, one
//! `listen` site) and the renderer can switch on `id` like any other
//! command bus. New menu items only need a new id literal on both
//! sides — no extra plumbing.
//!
//! Cross-platform notes:
//!  - `MenuBuilder::default` wires the platform-appropriate first
//!    submenu automatically (the "App" menu on macOS, none on Windows
//!    / Linux). We add `File / Edit / View / AI / Help` on top of
//!    that so the bar is consistent.
//!  - Edit + View pick up the predefined OS bindings (Cut, Copy,
//!    Paste, Select-All, Reload, Toggle `DevTools`) so the user gets
//!    the muscle-memory shortcuts for free without us managing
//!    accelerators on three platforms.

use tauri::{
    menu::{
        AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
    },
    AppHandle, Emitter, Runtime, WebviewWindow,
};

/// Stable IDs the renderer matches against. Keep this list in sync
/// with `apps/desktop/src/lib/app-menu-events.ts`.
pub mod ids {
    pub const FILE_OPEN_FOLDER: &str = "file/open-folder";
    pub const FILE_SETTINGS: &str = "file/settings";
    pub const VIEW_TOGGLE_SIDEBAR: &str = "view/toggle-sidebar";
    pub const VIEW_TOGGLE_AI_PANEL: &str = "view/toggle-ai-panel";
    pub const AI_ANALYZE: &str = "ai/analyze";
    pub const AI_REGENERATE: &str = "ai/regenerate";
    pub const HELP_DOCS: &str = "help/docs";
    pub const HELP_GITHUB: &str = "help/github";
}

/// Tauri event the menu emits when any item is clicked.
pub const MENU_EVENT: &str = "app:menu";

/// Build and attach the application menu to `window`.
///
/// # Errors
///
/// Returns the underlying Tauri error if menu construction or
/// attachment fails. The caller in `lib.rs` propagates this through
/// the `setup` callback so a malformed menu bricks startup loudly
/// instead of silently launching with no menu bar.
pub fn build_and_attach<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
) -> tauri::Result<()> {
    let about_meta = AboutMetadataBuilder::new()
        .name(Some("Tessera"))
        .version(Some(env!("CARGO_PKG_VERSION")))
        .website(Some("https://github.com/Rajveerx11/Tessera"))
        .website_label(Some("github.com/Rajveerx11/Tessera"))
        .comments(Some(
            "Local-first AI testing IDE — turn any codebase into a full QA dossier without sending source to the cloud.",
        ))
        .build();

    // File ----------------------------------------------------------
    let file_open_folder = MenuItemBuilder::with_id(ids::FILE_OPEN_FOLDER, "Open Folder…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let file_settings = MenuItemBuilder::with_id(ids::FILE_SETTINGS, "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let file_quit = PredefinedMenuItem::quit(app, None)?;
    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&file_open_folder)
        .separator()
        .item(&file_settings)
        .separator()
        .item(&file_quit)
        .build()?;

    // Edit ----------------------------------------------------------
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .build()?;

    // View ----------------------------------------------------------
    let view_toggle_sidebar = MenuItemBuilder::with_id(ids::VIEW_TOGGLE_SIDEBAR, "Toggle Sidebar")
        .accelerator("CmdOrCtrl+B")
        .build(app)?;
    let view_toggle_ai_panel =
        MenuItemBuilder::with_id(ids::VIEW_TOGGLE_AI_PANEL, "Toggle AI Panel")
            .accelerator("CmdOrCtrl+J")
            .build(app)?;
    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&view_toggle_sidebar)
        .item(&view_toggle_ai_panel)
        .separator()
        .item(&PredefinedMenuItem::fullscreen(app, None)?)
        .build()?;

    // AI ------------------------------------------------------------
    let ai_analyze = MenuItemBuilder::with_id(ids::AI_ANALYZE, "Analyze Project")
        .accelerator("CmdOrCtrl+Shift+A")
        .build(app)?;
    let ai_regenerate = MenuItemBuilder::with_id(ids::AI_REGENERATE, "Regenerate Last")
        .accelerator("CmdOrCtrl+G")
        .build(app)?;
    let ai_menu = SubmenuBuilder::new(app, "AI")
        .item(&ai_analyze)
        .item(&ai_regenerate)
        .build()?;

    // Help ----------------------------------------------------------
    let help_docs = MenuItemBuilder::with_id(ids::HELP_DOCS, "Documentation").build(app)?;
    let help_github = MenuItemBuilder::with_id(ids::HELP_GITHUB, "Open GitHub Repository")
        .accelerator("CmdOrCtrl+Shift+G")
        .build(app)?;
    let help_about = PredefinedMenuItem::about(app, Some("About Tessera"), Some(about_meta))?;
    let help_menu = SubmenuBuilder::new(app, "Help")
        .item(&help_docs)
        .item(&help_github)
        .separator()
        .item(&help_about)
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&ai_menu)
        .item(&help_menu)
        .build()?;

    window.set_menu(menu)?;

    // Single click handler — fans every custom id out as one
    // `app:menu` event. Predefined items (Cut / Copy / Paste / etc.)
    // do not fire this hook; the OS handles them directly.
    let handle = app.clone();
    app.on_menu_event(move |_app, event| {
        let id = event.id().as_ref().to_string();
        if let Err(error) = handle.emit(MENU_EVENT, id) {
            tracing::warn!(?error, "failed to emit menu event");
        }
    });

    Ok(())
}
