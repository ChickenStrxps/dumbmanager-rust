//! Opt-in local frameboards. Executables are configured by the user, never discovered.
use super::*;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const SETTINGS: &str = "media-frameboard-settings.json";
fn executable(raw: &str) -> LegacyResult<PathBuf> {
    let p = Path::new(raw);
    if raw.len() > 1024
        || !p.is_absolute()
        || !p.is_file()
        || fs::symlink_metadata(p).map_or(true, |m| m.file_type().is_symlink())
    {
        return Err(LegacyError::invalid(
            "Choose an absolute path to a local executable.",
        ));
    }
    #[cfg(windows)]
    if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
        return Err(LegacyError::invalid("Choose an executable file."));
    }
    fs::canonicalize(p).map_err(|_| LegacyError::invalid("Cannot resolve the executable."))
}
pub(super) fn configure(state: &AppState, args: &Value) -> LegacyResult<Value> {
    if args["confirm"] != true {
        return Err(LegacyError::conflict(
            "Confirm the FFmpeg and FFprobe executable paths.",
        ));
    }
    if args["clear"] == true {
        state.runtime.write_document(SETTINGS, &json!({}))?;
        return Ok(json!({"configured":false}));
    }
    let ffmpeg = executable(args["ffmpeg"].as_str().unwrap_or(""))?;
    let ffprobe = executable(args["ffprobe"].as_str().unwrap_or(""))?;
    state.runtime.write_document(
        SETTINGS,
        &json!({"ffmpeg":ffmpeg.to_string_lossy(),"ffprobe":ffprobe.to_string_lossy()}),
    )?;
    settings(state)
}
pub(super) fn settings(state: &AppState) -> LegacyResult<Value> {
    let v = state.runtime.read_document(SETTINGS)?;
    Ok(
        json!({"configured":v["ffmpeg"].as_str().is_some() && v["ffprobe"].as_str().is_some(),"ffmpeg":v["ffmpeg"],"ffprobe":v["ffprobe"]}),
    )
}
fn run(command: &mut Command) -> LegacyResult<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| LegacyError::unavailable("Could not start configured video tool."))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(LegacyError::invalid(
                        "Video conversion failed. Try a short MP4/WebM or use the original.",
                    ))
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(LegacyError::unavailable("Video conversion timed out."));
            }
        }
    }
}
struct Temp(PathBuf, PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let safe = fs::symlink_metadata(&self.0)
            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && self
                .0
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("frameboard-"))
            && fs::canonicalize(&self.0)
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                == fs::canonicalize(&self.1).ok();
        if safe {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
pub(super) fn convert(state: &AppState, bytes: &[u8]) -> LegacyResult<(Vec<u8>, f64)> {
    let cfg = state.runtime.read_document(SETTINGS)?;
    let ffmpeg = executable(cfg["ffmpeg"].as_str().unwrap_or(""))?;
    let ffprobe = executable(cfg["ffprobe"].as_str().unwrap_or(""))?;
    let base = folder(state.runtime.root())?;
    let dir = base.join(format!("frameboard-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&dir).map_err(|_| storage())?;
    let _temp = Temp(dir.clone(), base.clone());
    let input = dir.join("input");
    let report = dir.join("probe.json");
    let out = dir.join("frameboard.jpg");
    fs::write(&input, bytes).map_err(|_| storage())?;
    fs::write(
        dir.join("font.ttf"),
        include_bytes!("../../../../web/fonts/IBMPlexMono.ttf"),
    )
    .map_err(|_| storage())?;
    run(Command::new(ffprobe)
        .current_dir(&dir)
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
            "-o",
        ])
        .arg(&report)
        .arg(&input))?;
    let metadata = fs::metadata(&report)
        .map_err(|_| LegacyError::invalid("Could not inspect video duration."))?;
    if metadata.len() > 65536 {
        return Err(LegacyError::invalid("Invalid video duration."));
    }
    let probe: Value = serde_json::from_slice(&fs::read(&report).map_err(|_| storage())?)
        .map_err(|_| LegacyError::invalid("Could not inspect video duration."))?;
    let duration = probe["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| LegacyError::invalid("Could not inspect video duration."))?;
    if !duration.is_finite() || !(0.1..=120.0).contains(&duration) {
        return Err(LegacyError::invalid(
            "Trim the clip to 0.1–120 seconds or choose the original.",
        ));
    }
    let filter = format!("fps={:.8}:round=up,scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2:color=0x131411,setsar=1,drawtext=fontfile=font.ttf:text='%{{pts\\:hms}}':x=8:y=h-th-8:fontsize=17:fontcolor=white:box=1:boxcolor=black@0.8,tile=3x3:nb_frames=9:padding=6:margin=6:color=0x131411",9.0/duration);
    run(Command::new(ffmpeg)
        .current_dir(&dir)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-threads",
            "1",
            "-i",
        ])
        .arg(&input)
        .args([
            "-an",
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-q:v",
            "4",
            "-threads",
            "1",
        ])
        .arg(&out))?;
    let m = fs::symlink_metadata(&out)
        .map_err(|_| LegacyError::invalid("Video frameboard was not produced."))?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() == 0 || m.len() > 2 * 1024 * 1024 {
        return Err(LegacyError::invalid(
            "Video frameboard is invalid or too large.",
        ));
    }
    let data = fs::read(out).map_err(|_| storage())?;
    if !data.starts_with(&[0xff, 0xd8, 0xff]) || !data.ends_with(&[0xff, 0xd9]) {
        return Err(LegacyError::invalid("Video frameboard is not JPEG."));
    }
    Ok((data, duration))
}
