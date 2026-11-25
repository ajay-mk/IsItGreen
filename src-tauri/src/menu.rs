//! The macOS app menu: Tauri's default menu, with "Check for Updates…" below
//! "About IsItGreen", where Mac apps put it. Windows and Linux have no app
//! menu, so they keep Settings → Updates.

use tauri::{
    menu::{Menu, MenuEvent, MenuItem, MenuItemKind},
    AppHandle, Emitter, Runtime,
};

/// The menu item's id, and the event the window gets when it is chosen
/// (`CHECK_FOR_UPDATES_EVENT` in `src/lib/transport.ts`).
pub const CHECK_FOR_UPDATES: &str = "check-for-updates";

/// Tauri's default menu, with "Check for Updates…" after the app menu's
/// first item, "About IsItGreen".
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        let item = MenuItem::with_id(
            app,
            CHECK_FOR_UPDATES,
            "Check for Updates…",
            true,
            None::<&str>,
        )?;
        app_menu.insert(&item, 1)?;
    }
    Ok(menu)
}

/// Passes "Check for Updates…" on to the window, which runs the check and
/// shows its outcome.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    if *event.id() == CHECK_FOR_UPDATES {
        let _ = app.emit_to("main", CHECK_FOR_UPDATES, ());
    }
}
