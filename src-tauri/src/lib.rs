mod ax25;
mod agw;
mod aprsis;
mod kiss;
mod message_export;
mod message_import;
mod message_store;
mod runtime;
mod settings;
mod tnc;
mod webserver;

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

fn focus_or_show(window: tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

fn open_preferences_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("preferences-window") {
        focus_or_show(window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        "preferences-window",
        WebviewUrl::App("/#preferences".into()),
    )
    .title("APRSWolf Preferences")
    // Wide enough for two columns of sections; the page then fits the
    // height to its content (see fitWindowToContent in PreferencesWindow).
    .inner_size(1000.0, 640.0)
    .min_inner_size(620.0, 520.0)
    .resizable(true)
    .build()?;

    focus_or_show(window);
    Ok(())
}

// Async so the window is built off the main thread; a sync command that
// creates a window can deadlock on Windows.
#[tauri::command]
async fn open_preferences(app: AppHandle) -> Result<(), String> {
    open_preferences_window(&app).map_err(|e| e.to_string())
}

fn open_export_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("export-window") {
        focus_or_show(window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, "export-window", WebviewUrl::App("/#export".into()))
        .title("Export Messages")
        .inner_size(520.0, 560.0)
        .min_inner_size(440.0, 420.0)
        .resizable(true)
        .build()?;

    focus_or_show(window);
    Ok(())
}

/// File > Import Messages: pick a CSV export, validate all of it, warn
/// about short-lived packets past the retention window, then store it and
/// tell the windows to reload their lists.
fn import_messages(app: &AppHandle) {
    use tauri::Emitter;
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    fn report(app: &AppHandle, kind: MessageDialogKind, text: String) {
        app.dialog().message(text).title("Import Messages").kind(kind).show(|_| {});
    }

    fn store(app: &AppHandle, rows: &[message_import::ImportRow]) {
        match message_store::import_rows(app, rows) {
            Ok(summary) => {
                let _ = app.emit("history_imported", summary.added);
                let plural = |n: usize| if n == 1 { "" } else { "s" };
                let text = match (summary.added, summary.skipped) {
                    (0, skipped) => format!(
                        "Nothing new to import: {} already in your history.",
                        if skipped == 1 { "the message is".to_string() } else { format!("all {skipped} messages are") }
                    ),
                    (added, 0) => format!("Imported {added} message{}.", plural(added)),
                    (added, skipped) => format!(
                        "Imported {added} message{}. {skipped} {} already in your history and {} skipped.",
                        plural(added),
                        if skipped == 1 { "was" } else { "were" },
                        if skipped == 1 { "was" } else { "were" }
                    ),
                };
                report(app, MessageDialogKind::Info, text);
            }
            Err(err) => report(app, MessageDialogKind::Error, err),
        }
    }

    let app = app.clone();
    app.dialog()
        .file()
        .set_title("Import Messages")
        .add_filter("CSV", &["csv"])
        .pick_file(move |chosen| {
            let Some(path) = chosen.and_then(|p| p.into_path().ok()) else {
                return;
            };
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > message_import::MAX_FILE_BYTES {
                let limit = message_import::MAX_FILE_BYTES / (1024 * 1024);
                report(&app, MessageDialogKind::Error, format!("The file is larger than {limit} MB, so it was not imported."));
                return;
            }
            let text = match std::fs::read(&path).map(String::from_utf8) {
                Ok(Ok(text)) => text,
                Ok(Err(_)) => {
                    report(&app, MessageDialogKind::Error, "The file is not UTF-8 text, so it is not an APRSWolf export.".to_string());
                    return;
                }
                Err(err) => {
                    report(&app, MessageDialogKind::Error, format!("Could not read {}: {err}", path.display()));
                    return;
                }
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            let rows = match message_import::parse_csv(&text, now) {
                Ok(rows) => rows,
                Err(err) => {
                    report(&app, MessageDialogKind::Error, err);
                    return;
                }
            };
            if rows.is_empty() {
                report(&app, MessageDialogKind::Info, "The file holds no messages.".to_string());
                return;
            }
            let retention_days = settings::load_settings(app.clone()).short_lived_retention_days;
            let old = message_store::old_short_lived(&rows, retention_days, now);
            if old == 0 {
                store(&app, &rows);
                return;
            }
            let text = format!(
                "{old} position, weather, or other short-lived packet{} in this file {} older than {} day{}, the retention set in Preferences > Message History. Imported, {} kept for {} day{} from now and then removed.",
                if old == 1 { "" } else { "s" },
                if old == 1 { "is" } else { "are" },
                retention_days,
                if retention_days == 1 { "" } else { "s" },
                if old == 1 { "it is" } else { "they are" },
                retention_days,
                if retention_days == 1 { "" } else { "s" },
            );
            let confirm_app = app.clone();
            app.dialog()
                .message(text)
                .title("Import old short-lived packets?")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom("Import".to_string(), "Cancel".to_string()))
                .show(move |ok| {
                    if ok {
                        store(&confirm_app, &rows);
                    }
                });
        });
}

fn open_about_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("about-window") {
        focus_or_show(window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, "about-window", WebviewUrl::App("/#about".into()))
        .title("About APRSWolf")
        .inner_size(420.0, 600.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .build()?;

    focus_or_show(window);
    Ok(())
}

// Async for the same reason as open_preferences.
#[tauri::command]
async fn open_help(app: AppHandle) -> Result<(), String> {
    open_help_window(&app).map_err(|e| e.to_string())
}

fn open_help_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("help-window") {
        focus_or_show(window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, "help-window", WebviewUrl::App("/#help".into()))
        .title("APRSWolf Help")
        .inner_size(920.0, 640.0)
        .min_inner_size(760.0, 520.0)
        .resizable(true)
        .build()?;

    focus_or_show(window);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(tnc::AppState::default())
        .manage(aprsis::AprsIsState::default())
        .manage(runtime::RuntimeState::default())
        .manage(webserver::WebServerState::default())
        .setup(|app| {
            let settings = settings::load_settings(app.handle().clone());
            if settings.web_server_auto_start {
                let app_handle = app.handle().clone();
                let port = settings
                    .web_server_port
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|value| *value > 0)
                    .unwrap_or(webserver::DEFAULT_WEB_SERVER_PORT);

                tauri::async_runtime::spawn(async move {
                    let _ = webserver::enable_web_server_internal(app_handle, port).await;
                });
            }

            Ok(())
        })
        .menu(|handle| {
            let preferences = MenuItemBuilder::with_id("preferences", "Preferences...")
                .accelerator("CmdOrCtrl+,")
                .build(handle)?;
            let help = MenuItemBuilder::with_id("help", "APRSWolf Help")
                .accelerator("CmdOrCtrl+Shift+/")
                .build(handle)?;

            let export = MenuItemBuilder::with_id("export-messages", "Export Messages...")
                .accelerator("CmdOrCtrl+Shift+E")
                .build(handle)?;

            let about = MenuItemBuilder::with_id("about", "About APRSWolf").build(handle)?;

            let app_menu = SubmenuBuilder::new(handle, "APRSWolf")
                .item(&about)
                .separator()
                .item(&preferences)
                .separator()
                .services()
                .separator()
                .quit()
                .build()?;

            let import = MenuItemBuilder::with_id("import-messages", "Import Messages...")
                .accelerator("CmdOrCtrl+Shift+I")
                .build(handle)?;

            let file_menu = SubmenuBuilder::new(handle, "File")
                .item(&import)
                .item(&export)
                .build()?;

            let help_menu = SubmenuBuilder::new(handle, "Help")
                .item(&help)
                .build()?;

            MenuBuilder::new(handle)
                .item(&app_menu)
                .item(&file_menu)
                .item(&help_menu)
                .build()
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "preferences" {
                let _ = open_preferences_window(app);
            }
            if event.id().as_ref() == "about" {
                let _ = open_about_window(app);
            }
            if event.id().as_ref() == "help" {
                let _ = open_help_window(app);
            }
            if event.id().as_ref() == "export-messages" {
                let _ = open_export_window(app);
            }
            if event.id().as_ref() == "import-messages" {
                import_messages(app);
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            open_preferences,
            open_help,
            tnc::connect_tnc,
            tnc::disconnect_tnc,
            tnc::send_message,
            tnc::clear_queue,
            tnc::inspect_queue,
            tnc::get_manual_beacon_cooldown_remaining,
            aprsis::connect_aprsis,
            aprsis::disconnect_aprsis,
            settings::load_settings,
            settings::save_settings,
            message_store::load_received_messages,
            message_store::search_received_messages,
            message_store::search_sent_messages,
            message_store::clear_messages,
            message_store::restore_messages,
            message_store::load_sent_messages,
            message_store::export_messages,
            message_store::last_new_only_export,
            tnc::send_beacon,
            webserver::get_web_server_status,
            webserver::enable_web_server,
            webserver::disable_web_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
