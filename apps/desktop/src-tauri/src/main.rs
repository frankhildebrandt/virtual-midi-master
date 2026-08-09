#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use std::time::Duration;

use tauri_plugin_shell::ShellExt;
use virtual_midi_engine::{IpcRequest, IpcResponse, request as send_ipc};

#[tauri::command]
fn engine_request(app: tauri::AppHandle, request: IpcRequest) -> Result<IpcResponse, String> {
    if let Ok(response) = send_ipc(&request) {
        return Ok(response);
    }
    let command = app
        .shell()
        .sidecar("virtual-midi-engine")
        .map_err(|error| format!("Engine-Sidecar fehlt: {error}"))?;
    command
        .spawn()
        .map_err(|error| format!("Engine konnte nicht gestartet werden: {error}"))?;
    for _ in 0..30 {
        thread::sleep(Duration::from_millis(100));
        if let Ok(response) = send_ipc(&request) {
            return Ok(response);
        }
    }
    Err("Die MIDI-Engine antwortet nach dem Start nicht. Bitte Runtime und Logs prüfen.".into())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![engine_request])
        .run(tauri::generate_context!())
        .expect("error while running Virtual MIDI Master");
}
