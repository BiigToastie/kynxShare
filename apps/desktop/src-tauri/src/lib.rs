mod commands;

use kynx_core::{AppConfig, KynxEngine};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub engine: Arc<KynxEngine>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kynxshare=info".parse().unwrap()))
        .init();

    let config = AppConfig::load().unwrap_or_default();
    let engine = Arc::new(KynxEngine::new(config).expect("engine init"));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            engine: Arc::clone(&engine),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_config,
            commands::apply_config,
            commands::persist_config,
            commands::save_config,
            commands::ensure_preview,
            commands::start_engine,
            commands::stop_engine,
            commands::set_output_active,
            commands::toggle_mode,
            commands::refresh_monitors,
            commands::apply_desktop_layout,
            commands::get_preview,
            commands::get_output_preview,
            commands::get_vdd_status,
            commands::open_vdd_installer,
        ])
        .setup(|app| {
            setup_tray(app.handle())?;
            let engine = app.state::<AppState>().engine.clone();
            std::thread::spawn(move || {
                if let Err(e) = engine.ensure_preview() {
                    tracing::warn!("auto preview failed: {e}");
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray instead of quitting
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running kynxShare");
}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Open kynxShare", true, None::<&str>)?;
    let toggle_out = MenuItem::with_id(app, "toggle_output", "Toggle Output", true, None::<&str>)?;
    let toggle_mode = MenuItem::with_id(app, "toggle_mode", "Toggle Mode", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle_out, &toggle_mode, &quit])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("kynxShare")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "toggle_output" => {
                let state = app.state::<AppState>();
                let active = !state.engine.status().output_active;
                state.engine.set_output_active(active);
            }
            "toggle_mode" => {
                let state = app.state::<AppState>();
                let _ = state.engine.toggle_mode();
            }
            "quit" => {
                let state = app.state::<AppState>();
                state.engine.stop();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
