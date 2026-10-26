//! Development-only desktop QA. No arguments, seeded data or normal app store.
use std::process::ExitCode;

fn main() -> ExitCode {
    match dumbmanager_rust_lib::native_ui_qa::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Native UI QA could not start: {error}");
            ExitCode::FAILURE
        }
    }
}
