mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use common::Fixture;
use dumbmanager_core::export::write_new_file;
use dumbmanager_core::{
    export_card, CardFormat, CardRenderer, CardRequest, CoreError, ExportJob, RenderedFile, Theme,
    CARD_RENDERER_UNAVAILABLE,
};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot-a-real-image-but-right-signature";
const GIF: &[u8] = b"GIF89a-test-frames";

/// Test double standing in for the parent's renderer. It writes the
/// configured bytes into the folder it is given (like the real one does),
/// records each request and reports whatever it was told to report.
struct StubRenderer {
    bytes: Vec<u8>,
    size: (u32, u32, u32),
    /// Extension to write; `None` uses the requested format's.
    extension: Option<&'static str>,
    /// Report this path instead of the file actually written.
    report: Option<PathBuf>,
    fail: Option<String>,
    seen: Mutex<Vec<(String, CardFormat, Theme, PathBuf)>>,
}

impl StubRenderer {
    fn writing(bytes: &[u8], width: u32, height: u32, frames: u32) -> Self {
        Self {
            bytes: bytes.to_vec(),
            size: (width, height, frames),
            extension: None,
            report: None,
            fail: None,
            seen: Mutex::new(Vec::new()),
        }
    }

    fn failing(message: &str) -> Self {
        Self {
            fail: Some(message.to_owned()),
            ..Self::writing(PNG, 1, 1, 1)
        }
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl CardRenderer for StubRenderer {
    fn render_into(
        &self,
        request: CardRequest<'_>,
        directory: &Path,
    ) -> Result<RenderedFile, String> {
        self.seen.lock().unwrap().push((
            request.itch.title.clone(),
            request.format,
            request.theme,
            directory.to_path_buf(),
        ));
        if let Some(message) = &self.fail {
            return Err(message.clone());
        }
        let extension = self.extension.unwrap_or(request.format.extension());
        let written = write_new_file(directory, "stub-card", extension, &self.bytes)
            .map_err(|error| error.to_string())?;
        let (width, height, frames) = self.size;
        Ok(RenderedFile {
            path: self.report.clone().unwrap_or(written),
            width,
            height,
            frames,
        })
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(entries) => {
            let mut names: Vec<_> = entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
        Err(_) => Vec::new(),
    }
}

#[test]
fn without_renderer_export_is_explicitly_unavailable_and_touches_nothing() {
    let mut fx = Fixture::new();
    let itch = fx.add("Card me", "idea", "p");
    let err = export_card(&fx.store, &fx.paths, None, &itch.id, "gif").unwrap_err();
    assert_eq!(
        err,
        CoreError::Unavailable(CARD_RENDERER_UNAVAILABLE.to_owned())
    );
    assert!(
        !fx.paths.exports_dir().exists(),
        "not even the exports folder is created"
    );
}

#[test]
fn bad_format_and_unknown_itch_are_reported_before_unavailable() {
    let mut fx = Fixture::new();
    let itch = fx.add("Card me", "idea", "p");
    for format in ["jpg", "GIF", "", "../png"] {
        assert!(matches!(
            export_card(&fx.store, &fx.paths, None, &itch.id, format),
            Err(CoreError::Invalid(_))
        ));
    }
    assert!(matches!(
        export_card(
            &fx.store,
            &fx.paths,
            None,
            "0f8fad5b-d9cb-469f-a165-70867728950e",
            "png"
        ),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        export_card(&fx.store, &fx.paths, None, "not-an-id", "png"),
        Err(CoreError::Invalid(_))
    ));
}

#[test]
fn renderer_writes_into_the_exports_folder_with_fresh_names() {
    let mut fx = Fixture::new();
    let itch = fx.add("Shipped dark mode", "improvement", "p");
    let renderer = StubRenderer::writing(PNG, 2400, 1350, 1);

    let first = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "png").unwrap();
    let second = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "png").unwrap();

    assert_ne!(
        first.path, second.path,
        "exports never overwrite each other"
    );
    for export in [&first, &second] {
        let path = Path::new(&export.path);
        assert_eq!(path.parent().unwrap(), fx.paths.exports_dir());
        assert_eq!(fs::read(path).unwrap(), PNG);
        assert_eq!(
            export.bytes,
            PNG.len() as u64,
            "size comes from the file on disk"
        );
        assert_eq!(
            (export.width, export.height, export.frames),
            (2400, 1350, 1)
        );
    }
    assert_eq!(
        files_in(&fx.paths.exports_dir()),
        vec!["stub-card-2.png", "stub-card.png"]
    );
    let seen = renderer.seen.lock().unwrap();
    assert_eq!(
        seen[0],
        (
            "Shipped dark mode".to_owned(),
            CardFormat::Png,
            Theme::Midnight,
            fx.paths.exports_dir()
        )
    );
}

#[test]
fn gif_export_reports_frames() {
    let mut fx = Fixture::new();
    let itch = fx.add("Looping card", "idea", "p");
    let renderer = StubRenderer::writing(GIF, 1200, 675, 72);
    let export = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "gif").unwrap();
    assert!(export.path.ends_with(".gif"));
    assert_eq!(
        (export.width, export.height, export.frames),
        (1200, 675, 72)
    );
}

#[test]
fn prepared_job_runs_without_the_store() {
    let mut fx = Fixture::new();
    let itch = fx.add("Detached", "idea", "p");
    let job = ExportJob::prepare(&fx.store, &itch.id, "png").unwrap();
    assert_eq!(job.itch(), &itch);
    assert_eq!(job.format(), CardFormat::Png);
    // The store can be closed while the slow part runs.
    let Fixture { store, paths, dir } = fx;
    drop(store);
    let renderer = StubRenderer::writing(PNG, 10, 10, 1);
    let export = job.run(&paths, Some(&renderer)).unwrap();
    assert!(Path::new(&export.path).is_file());
    drop(dir);
}

#[test]
fn unusable_renderer_output_is_rejected_and_deleted() {
    let mut fx = Fixture::new();
    let itch = fx.add("Broken renderer", "bug", "p");
    let mut wrong_extension = StubRenderer::writing(PNG, 10, 10, 1);
    wrong_extension.extension = Some("gif");
    let cases = [
        StubRenderer::writing(b"", 10, 10, 1),
        StubRenderer::writing(GIF, 10, 10, 1), // GIF bytes for a png request
        StubRenderer::writing(PNG, 0, 10, 1),
        StubRenderer::writing(PNG, 10, 9_000, 1),
        StubRenderer::writing(PNG, 10, 10, 0),
        StubRenderer::writing(PNG, 10, 10, 2), // a PNG has exactly one frame
        wrong_extension,
    ];
    for renderer in &cases {
        let err = export_card(&fx.store, &fx.paths, Some(renderer), &itch.id, "png").unwrap_err();
        assert!(matches!(err, CoreError::Render(_)), "{err:?}");
        assert!(err.to_string().contains("Nothing was exported"), "{err}");
        assert_eq!(renderer.calls(), 1);
    }
    assert!(
        files_in(&fx.paths.exports_dir()).is_empty(),
        "rejected files are removed"
    );
}

#[test]
fn renderer_errors_are_reported_with_context() {
    let mut fx = Fixture::new();
    let itch = fx.add("x", "bug", "p");
    let renderer = StubRenderer::failing("font table missing.");
    let err = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "gif").unwrap_err();
    assert_eq!(
        err.to_string(),
        "The card renderer failed: font table missing. Nothing was exported."
    );
}

#[test]
fn files_reported_outside_the_exports_folder_are_refused_and_left_alone() {
    let mut fx = Fixture::new();
    let itch = fx.add("Escaping renderer", "bug", "p");
    let outside = fx.dir.path().join("elsewhere.png");
    fs::write(&outside, PNG).unwrap();
    let nested_dir = fx.paths.exports_dir().join("nested");

    let mut outside_report = StubRenderer::writing(PNG, 10, 10, 1);
    outside_report.report = Some(outside.clone());
    let mut relative_report = StubRenderer::writing(PNG, 10, 10, 1);
    relative_report.report = Some(PathBuf::from("stub-card.png"));
    let mut missing_report = StubRenderer::writing(PNG, 10, 10, 1);
    missing_report.report = Some(fx.paths.exports_dir().join("never-written.png"));
    let mut nested_report = StubRenderer::writing(PNG, 10, 10, 1);
    nested_report.report = Some(nested_dir.join("x.png"));
    let mut folder_report = StubRenderer::writing(PNG, 10, 10, 1);
    folder_report.report = Some(nested_dir.clone());

    fx.paths.ensure_exports_dir().unwrap();
    fs::create_dir(&nested_dir).unwrap();
    fs::write(nested_dir.join("x.png"), PNG).unwrap();

    for renderer in [
        &outside_report,
        &relative_report,
        &missing_report,
        &nested_report,
        &folder_report,
    ] {
        let err = export_card(&fx.store, &fx.paths, Some(renderer), &itch.id, "png").unwrap_err();
        assert!(matches!(err, CoreError::Render(_)), "{err:?}");
    }
    assert_eq!(
        fs::read(&outside).unwrap(),
        PNG,
        "never deletes outside the exports folder"
    );
    assert!(
        nested_dir.join("x.png").is_file(),
        "never deletes in subfolders"
    );
}

#[test]
fn exports_path_that_is_a_file_is_refused_before_rendering() {
    let mut fx = Fixture::new();
    let itch = fx.add("x", "bug", "p");
    fs::write(fx.paths.exports_dir(), b"not a folder").unwrap();
    let renderer = StubRenderer::writing(PNG, 10, 10, 1);
    let err = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "png").unwrap_err();
    assert!(err.to_string().contains("not a folder"), "{err}");
    assert_eq!(renderer.calls(), 0);
}

/// Creates a directory link (symlink, or a junction on Windows when symlinks
/// need privileges). Returns false if the platform refuses both.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn linked_exports_folder_is_refused() {
    let mut fx = Fixture::new();
    let itch = fx.add("x", "bug", "p");
    let outside = fx.dir.path().join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    if !link_dir(&outside, &fx.paths.exports_dir()) {
        eprintln!("skipping: this machine cannot create directory links");
        return;
    }
    let renderer = StubRenderer::writing(PNG, 10, 10, 1);
    let err = export_card(&fx.store, &fx.paths, Some(&renderer), &itch.id, "png").unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)), "{err:?}");
    assert_eq!(renderer.calls(), 0);
    assert!(
        fs::read_dir(&outside).unwrap().next().is_none(),
        "nothing written through the link"
    );
    assert!(fx.paths.ensure_exports_dir().is_err());
}

#[test]
fn write_new_file_rejects_unsafe_names_and_never_overwrites() {
    let dir = common::TestDir::new();
    for (stem, ext) in [
        ("../escape", "png"),
        ("a/b", "png"),
        ("ok", "p.ng"),
        ("", "png"),
        ("c:", "png"),
    ] {
        assert!(
            write_new_file(dir.path(), stem, ext, b"x").is_err(),
            "{stem}.{ext}"
        );
    }
    let first = write_new_file(dir.path(), "card", "png", b"one").unwrap();
    let second = write_new_file(dir.path(), "card", "png", b"two").unwrap();
    assert_eq!(first.file_name().unwrap(), "card.png");
    assert_eq!(second.file_name().unwrap(), "card-2.png");
    assert_eq!(fs::read(&first).unwrap(), b"one");
    assert_eq!(fs::read(&second).unwrap(), b"two");
    assert!(files_in(dir.path())
        .iter()
        .all(|f| !f.ends_with(".partial")));
}
