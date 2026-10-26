// Release builds are GUI apps: no console window next to the main window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dumbmanager_rust_lib::run();
}
