#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if music_auto_sync_lib::cli::run_cli() {
        return;
    }
    music_auto_sync_lib::run();
}
