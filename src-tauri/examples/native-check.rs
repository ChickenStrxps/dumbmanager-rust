//! Console-only native QA. Does not start the desktop app or open normal data.
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let root = match (arguments.next(), arguments.next()) {
        (Some(root), None) => PathBuf::from(root),
        _ => {
            eprintln!("Usage: native-check <new-direct-child-of-OS-temp-directory>");
            return ExitCode::from(2);
        }
    };
    match dumbmanager_rust_lib::native_check::run(&root) {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("Could not serialize native QA report: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
