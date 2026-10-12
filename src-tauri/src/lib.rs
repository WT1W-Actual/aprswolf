mod ax25;
mod agw;
mod aprsis;
mod kiss;
mod message_store;
mod runtime;
mod settings;
mod tnc;
mod webserver;

use tauri::{
    menu::{AboutMetadata, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
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
    .inner_size(720.0, 620.0)
    .min_inner_size(620.0, 520.0)
    .resizable(true)
    .build()?;

    focus_or_show(window);
    Ok(())
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

            let app_menu = SubmenuBuilder::new(handle, "APRSWolf")
                .item(&preferences)
                .separator()
                .services()
                .about(Some(AboutMetadata::default()))
                .quit()
                .build()?;

            let help_menu = SubmenuBuilder::new(handle, "Help")
                .item(&help)
                .build()?;

            MenuBuilder::new(handle)
                .item(&app_menu)
                .item(&help_menu)
                .build()
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "preferences" {
                let _ = open_preferences_window(app);
            }
            if event.id().as_ref() == "help" {
                let _ = open_help_window(app);
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
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
            tnc::send_beacon,
            webserver::get_web_server_status,
            webserver::enable_web_server,
            webserver::disable_web_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
