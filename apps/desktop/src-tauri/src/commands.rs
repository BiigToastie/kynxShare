use crate::AppState;
use kynx_capture::MonitorInfo;
use kynx_compositor::OutputMode;
use kynx_core::{AppConfig, EngineSnapshot};
use kynx_output::VirtualDisplayStatus;
use tauri::State;

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> EngineSnapshot {
    state.engine.snapshot()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.engine.get_config()
}

/// Live-apply config (preview updates) without writing disk.
#[tauri::command]
pub fn apply_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    state.engine.apply_config(config).map_err(|e| e.to_string())
}

/// Persist current engine config to disk.
#[tauri::command]
pub fn persist_config(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.persist_config().map_err(|e| e.to_string())
}

/// Apply + persist (used by Speichern with explicit config payload).
#[tauri::command]
pub fn save_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    state.engine.update_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ensure_preview(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.ensure_preview().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.stop();
    Ok(())
}

#[tauri::command]
pub fn set_output_active(state: State<'_, AppState>, active: bool) -> Result<(), String> {
    state.engine.set_output_active(active);
    Ok(())
}

#[tauri::command]
pub fn toggle_mode(state: State<'_, AppState>) -> Result<OutputMode, String> {
    state.engine.toggle_mode().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn refresh_monitors(state: State<'_, AppState>) -> Result<Vec<MonitorInfo>, String> {
    state.engine.refresh_monitors().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_desktop_layout(state: State<'_, AppState>) -> Result<(), String> {
    state
        .engine
        .apply_desktop_layout()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_preview(state: State<'_, AppState>) -> Option<String> {
    state
        .engine
        .layout_preview_jpeg()
        .map(|b| format!("data:image/jpeg;base64,{}", base64_encode(&b)))
}

#[tauri::command]
pub fn get_output_preview(state: State<'_, AppState>) -> Option<String> {
    state
        .engine
        .output_preview_jpeg()
        .map(|b| format!("data:image/jpeg;base64,{}", base64_encode(&b)))
}

#[tauri::command]
pub fn get_vdd_status(state: State<'_, AppState>) -> VirtualDisplayStatus {
    state.engine.vdd_status()
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let c = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (a << 16) | (b << 8) | c;
        out.push(T[((triple >> 18) & 63) as usize] as char);
        out.push(T[((triple >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((triple >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(triple & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
