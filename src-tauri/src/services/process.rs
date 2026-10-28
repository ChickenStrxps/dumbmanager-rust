//! Bounded argv-based child processes. No shell interpolation or inherited Git overrides.
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use std::{
    ffi::OsStr,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: i32,
}
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

pub fn run(
    program: &OsStr,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> LegacyResult<Output> {
    let mut command = Command::new(program);
    let mut stdout = tempfile::tempfile()
        .map_err(|_| LegacyError::unavailable("Could not prepare command output."))?;
    let mut stderr = tempfile::tempfile()
        .map_err(|_| LegacyError::unavailable("Could not prepare command output."))?;
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(
            stdout
                .try_clone()
                .map_err(|_| LegacyError::unavailable("Could not prepare command output."))?,
        )
        .stderr(
            stderr
                .try_clone()
                .map_err(|_| LegacyError::unavailable("Could not prepare command output."))?,
        );
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| LegacyError {
        error: "The configured command could not start. Check that it is installed.".into(),
        code: "process_unavailable".into(),
        status: 503,
    })?;
    let start = Instant::now();
    let mut failure = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => {
                failure = Some("Could not read the command result.");
                break None;
            }
        }
        if stdout.metadata().map_or(true, |m| m.len() > MAX_OUTPUT)
            || stderr.metadata().map_or(true, |m| m.len() > MAX_OUTPUT)
        {
            failure = Some("Command output exceeded the supported size.");
            break None;
        }
        if start.elapsed() > timeout {
            failure = Some("The command timed out. Refresh its status before retrying a write.");
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(message) = failure {
        return Err(LegacyError {
            error: message.into(),
            code: "process_incomplete".into(),
            status: 504,
        });
    }
    fn read(file: &mut std::fs::File) -> LegacyResult<Vec<u8>> {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| LegacyError::unavailable("Could not read command output."))?;
        let mut bytes = Vec::new();
        file.take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| LegacyError::unavailable("Could not read command output."))?;
        if bytes.len() as u64 > MAX_OUTPUT {
            return Err(LegacyError {
                error: "Command output exceeded the supported size.".into(),
                code: "process_too_large".into(),
                status: 413,
            });
        }
        Ok(bytes)
    }
    Ok(Output {
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
        code: status.and_then(|s| s.code()).unwrap_or(-1),
    })
}
