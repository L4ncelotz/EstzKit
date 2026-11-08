//! EstzKit Tauri application entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    estzkit_tauri_lib::run()
}
