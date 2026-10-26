use dumbmanager_card_renderer::{export_card, CardInput};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "artifacts/cards".into());
    let card=CardInput {title:"An itch, with its own workspace",description:"Native storage, a dedicated desktop window and animated ASCII cards. Everything stays separate from the original app.",project:"DumbManager Rust",status:"ready",seed:"native-rust-koi"};
    for format in ["png", "gif"] {
        let start = std::time::Instant::now();
        let out = export_card(&dir, &card, format)?;
        println!(
            "{} {}x{} {} frames {} bytes {:.1} ms",
            out.path.display(),
            out.width,
            out.height,
            out.frames,
            out.bytes,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
