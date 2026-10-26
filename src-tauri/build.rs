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
    "legacy_api",
];

fn main() {
    if std::env::var_os("CARGO_FEATURE_DESKTOP_QA").is_some() {
        assert_eq!(
            std::env::var("PROFILE").as_deref(),
            Ok("debug"),
            "desktop-qa is development-only; omit this feature from release builds"
        );
    }
    println!("cargo:rerun-if-changed=../assets/icons");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");

    // tauri-winres links resources into binaries, but Cargo examples need an
    // explicit link too. In particular, the Common Controls v6 manifest is
    // required for TaskDialogIndirect before the QA executable can start.
    if std::env::var_os("CARGO_FEATURE_DESKTOP_QA").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let resource =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("resource.lib");
        assert!(
            resource.is_file(),
            "Windows QA resources were not generated"
        );
        println!("cargo:rustc-link-arg-examples={}", resource.display());
    }
}
