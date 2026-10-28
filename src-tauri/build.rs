/// Every app command is listed here so tauri-build generates `allow-<command>`
/// permissions for it. Because an app manifest exists, a command is callable
/// only where a capability grants it (see `capabilities/main.json`), and the
/// capabilities grant nothing to remote origins.
const COMMANDS: &[&str] = &[
    "app_snapshot",
    "itch_list",
    "itch_save",
    "itch_transition",
    "settings_get",
    "settings_save",
    "card_export",
    "open_export_folder",
    "choose_path",
    "legacy_api",
];

fn main() {
    println!("cargo:rerun-if-changed=../assets/icons");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
