//! Disco Code desktop shell.
//!
//! A thin Tauri host around the same Rust core the CLI uses. The window is the
//! product surface; all behaviour lives in the `runtime` and `api` crates, so
//! the desktop app and the CLI cannot drift in what they actually do.

// The webview is the interface, so a console window alongside it is noise on
// Windows. Debug builds keep it: that is where panics become visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::daemon_status,
            commands::triage_request,
            commands::send_prompt,
            commands::cancel_turn,
        ])
        .run(tauri::generate_context!())
        .expect("the desktop shell failed to start");
}
