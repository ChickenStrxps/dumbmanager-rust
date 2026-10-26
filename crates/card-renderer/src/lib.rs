//! Native indexed-color cards: no web server, external process or font download.
use font8x8::{UnicodeFonts, BASIC_FONTS, LATIN_FONTS};
use std::{
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, BufWriter},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const WIDTH: u16 = 1200;
pub const HEIGHT: u16 = 675;
pub const FRAMES: usize = 72;
const TAU: f64 = std::f64::consts::TAU;
const PALETTE: &[u8] = &[
    9, 10, 16, 16, 18, 28, 25, 29, 43, 44, 45, 65, 91, 88, 119, 151, 147, 173, 234, 231, 244, 255,
    255, 255, 255, 74, 199, 146, 50, 124, 54, 218, 207, 24, 104, 109, 128, 237, 213, 59, 69, 95,
    238, 182, 115, 113, 81, 136,
];

#[derive(Clone, Debug)]
pub struct CardInput<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub project: &'a str,
    pub status: &'a str,
    pub seed: &'a str,
}

#[derive(Debug)]
pub struct Export {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    pub bytes: u64,
}

fn error(value: impl ToString) -> io::Error {
    io::Error::other(value.to_string())
}

struct Canvas {
    pixels: Vec<u8>,
}
impl Canvas {
    fn new() -> Self {
        Self {
            pixels: vec![0; WIDTH as usize * HEIGHT as usize],
        }
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        for yy in y.max(0)..(y + h).min(HEIGHT as i32) {
            let left = x.max(0).min(WIDTH as i32) as usize;
            let right = (x + w).max(0).min(WIDTH as i32) as usize;
            if right > left {
                self.pixels
                    [yy as usize * WIDTH as usize + left..yy as usize * WIDTH as usize + right]
                    .fill(color);
            }
        }
    }
    fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, color: u8) {
        for (i, ch) in text.chars().enumerate() {
            let glyph = BASIC_FONTS
                .get(ch)
                .or_else(|| LATIN_FONTS.get(ch))
                .unwrap_or_else(|| BASIC_FONTS.get('?').unwrap());
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) != 0 {
                        self.rect(
                            x + i as i32 * 9 * scale + col * scale,
                            y + row as i32 * scale,
                            scale,
                            scale,
                            color,
                        );
                    }
                }
            }
        }
    }
}

/// Word wrapping never splits a Unicode code point or produces an overwide line.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            result.push(std::mem::take(&mut line));
        }
        for ch in word.chars() {
            if line.chars().count() >= width {
                result.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
        if line.chars().count() < width {
            line.push(' ');
        }
    }
    if !line.trim().is_empty() {
        result.push(line.trim_end().to_owned());
    }
    result
        .into_iter()
        .map(|v| v.trim_end().to_owned())
        .collect()
}

fn seed(text: &str) -> u64 {
    text.bytes().fold(1469598103934665603, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}

fn base(input: &CardInput<'_>) -> Canvas {
    let mut c = Canvas::new();
    c.rect(20, 20, 1160, 635, 2);
    c.rect(21, 21, 1158, 633, 0);
    c.rect(20, 20, 5, 70, 8);
    c.rect(20, 20, 100, 3, 8);
    c.rect(1080, 652, 100, 3, 10);
    c.rect(1176, 585, 4, 70, 10);
    for y in (34..640).step_by(16) {
        for x in (38..1170).step_by(16) {
            c.rect(x, y, 1, 1, 2);
        }
    }
    c.text(52, 52, "[ DM ]", 2, 8);
    c.text(188, 56, "BUILD LOG / RUST", 1, 5);
    c.rect(52, 94, 1092, 1, 3);
    let project = wrap(input.project, 32)
        .first()
        .cloned()
        .unwrap_or_else(|| "UNSORTED".into());
    c.text(52, 120, &project.to_uppercase(), 1, 10);
    let large_title = wrap(input.title, 24);
    let (titles, scale, line_height) = if large_title.len() <= 4 {
        (large_title, 3, 38)
    } else {
        (wrap(input.title, 35), 2, 26)
    };
    for (i, line) in titles.iter().enumerate() {
        c.text(50, 158 + i as i32 * line_height, line, scale, 6);
    }
    let start = 172 + titles.len() as i32 * line_height;
    let description = wrap(input.description, 46);
    let description_lines = ((550 - start) / 23).clamp(0, 5) as usize;
    for (i, line) in description.iter().take(description_lines).enumerate() {
        c.text(52, start + i as i32 * 23, line, 1, 5);
    }
    if description.len() > description_lines {
        c.text(
            52,
            start + description_lines as i32 * 23,
            "Full details stay with the itch.",
            1,
            4,
        );
    }
    c.rect(52, 578, 1092, 1, 3);
    let label = match input.status {
        "resolved" => "ACCEPTED",
        "ready" => "READY FOR REVIEW",
        "stashed" => "STASHED",
        _ => "OPEN ITCH",
    };
    c.text(52, 608, &format!("[ + ] {label}"), 1, 10);
    c.text(795, 608, "DUMBMANAGER RUST", 1, 4);
    c
}

fn animate(c: &mut Canvas, phase: f64, salt: u64) {
    // Continuous periodic fields. The content column never moves or flickers.
    for row in 0..48 {
        for col in 0..46 {
            let x = 718 + col * 9;
            let y = 142 + row * 8;
            let xx = col as f64;
            let yy = row as f64;
            let flow = (xx * 0.13 + (yy * 0.1 + phase).sin() * 1.7 - phase).sin();
            let bubble = ((xx - 22.0).powi(2) / 360.0 + (yy - 23.0).powi(2) / 700.0).sqrt();
            if flow > 0.84 && bubble < 1.0 {
                let glyph = if flow > 0.97 {
                    "+"
                } else if flow > 0.92 {
                    ":"
                } else {
                    "."
                };
                c.text(x, y, glyph, 1, if flow > 0.97 { 11 } else { 2 });
            }
        }
    }
    // A dense koi silhouette built from thousands of original bitmap strokes.
    let cx = 928.0 + phase.sin() * 22.0;
    let cy = 318.0 + (phase + 0.7).cos() * 16.0;
    for row in -15..16 {
        for col in -21..22 {
            let dx = col as f64;
            let dy = row as f64;
            let body = (dx / 19.0).powi(2) + (dy / 10.0).powi(2);
            let tail = dx < -12.0 && dy.abs() < (-dx - 11.0) * 0.7 && dx > -24.0;
            if body < 1.0 || tail {
                let wave = (dy * 0.5 + dx * 0.31 + phase * 2.0).sin();
                let color = if body > 0.83 {
                    10
                } else if (dx + dy * 0.7 + phase.sin() * 2.0).sin() > 0.25 {
                    8
                } else {
                    if wave > 0.0 {
                        12
                    } else {
                        11
                    }
                };
                let ch = if body < 0.8 {
                    if wave > 0.5 {
                        "@"
                    } else if wave > -0.3 {
                        "+"
                    } else {
                        ":"
                    }
                } else {
                    "."
                };
                c.text(
                    (cx + dx * 7.0) as i32,
                    (cy + dy * 9.0 + (dx * 0.15 + phase).sin() * 5.0) as i32,
                    ch,
                    1,
                    color,
                );
            }
        }
    }
    c.text((cx + 100.0) as i32, cy as i32 - 8, "o", 1, 7);
    // Small squid and a seeded cameo live in the lower right of the frame.
    let squid_y = 474.0 + phase.cos() * 10.0;
    c.text(820, squid_y as i32, ".---.", 1, 8);
    c.text(811, squid_y as i32 + 10, "( o o )", 1, 8);
    c.text(820, squid_y as i32 + 20, "|:::|", 1, 9);
    for n in 0..5 {
        c.text(
            819 + n * 9 + (phase + n as f64).sin().round() as i32 * 2,
            squid_y as i32 + 30 + (n % 2) * 4,
            "~",
            1,
            9,
        );
    }
    if salt.is_multiple_of(7) {
        c.text(1030, 500, "(_)", 1, 14);
        c.text(1021, 512, "/|]", 1, 5);
        c.text(1021, 524, r"/ \", 1, 5);
    }
    for n in 0..7 {
        let h = (phase + n as f64 * 0.8).sin();
        c.text(739 + n * 58, 555 + (h * 4.0) as i32, "~", 1, 11);
    }
}

pub fn render_frame(input: &CardInput<'_>, phase: f64) -> Vec<u8> {
    let mut c = base(input);
    animate(&mut c, phase.rem_euclid(TAU), seed(input.seed));
    c.pixels
}

/// Writes a new, uniquely named export only within the supplied export directory.
/// Limits are fixed to prevent unbounded image sizes or frame counts.
pub fn export_card(directory: &Path, input: &CardInput<'_>, format: &str) -> io::Result<Export> {
    if !matches!(format, "gif" | "png") {
        return Err(error("Choose gif or png."));
    }
    if input.title.trim().is_empty()
        || input.title.chars().count() > 200
        || input.description.chars().count() > 20_000
    {
        return Err(error(
            "A title up to 200 characters and description up to 20,000 characters are required.",
        ));
    }
    fs::create_dir_all(directory)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let path = directory.join(format!(
        "dm-rust-{stamp}-{salt:016x}.{format}",
        salt = seed(input.seed)
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = (|| -> io::Result<(u32, u32, usize)> {
        let mut writer = BufWriter::new(file);
        if format == "gif" {
            let mut encoder =
                gif::Encoder::new(&mut writer, WIDTH, HEIGHT, PALETTE).map_err(error)?;
            encoder.set_repeat(gif::Repeat::Infinite).map_err(error)?;
            let static_pixels = base(input).pixels;
            for i in 0..FRAMES {
                let mut c = Canvas {
                    pixels: static_pixels.clone(),
                };
                animate(&mut c, i as f64 / FRAMES as f64 * TAU, seed(input.seed));
                let frame = gif::Frame {
                    width: WIDTH,
                    height: HEIGHT,
                    delay: 5,
                    dispose: gif::DisposalMethod::Keep,
                    buffer: Cow::Owned(c.pixels),
                    ..Default::default()
                };
                encoder.write_frame(&frame).map_err(error)?;
            }
            drop(encoder);
            std::io::Write::flush(&mut writer)?;
            Ok((WIDTH as u32, HEIGHT as u32, FRAMES))
        } else {
            let pixels = render_frame(input, 0.0);
            let mut doubled = vec![0u8; WIDTH as usize * HEIGHT as usize * 4];
            let out_width = WIDTH as usize * 2;
            for y in 0..HEIGHT as usize {
                for x in 0..WIDTH as usize {
                    let p = pixels[y * WIDTH as usize + x];
                    let i = y * 2 * out_width + x * 2;
                    doubled[i] = p;
                    doubled[i + 1] = p;
                    doubled[i + out_width] = p;
                    doubled[i + out_width + 1] = p;
                }
            }
            let mut encoder = png::Encoder::new(&mut writer, WIDTH as u32 * 2, HEIGHT as u32 * 2);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(PALETTE);
            let mut png = encoder.write_header().map_err(error)?;
            png.write_image_data(&doubled).map_err(error)?;
            png.finish().map_err(error)?;
            std::io::Write::flush(&mut writer)?;
            Ok((WIDTH as u32 * 2, HEIGHT as u32 * 2, 1))
        }
    })();
    match result {
        Ok((width, height, frames)) => Ok(Export {
            bytes: fs::metadata(&path)?.len(),
            path,
            width,
            height,
            frames,
        }),
        Err(e) => {
            let _ = fs::remove_file(&path);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> CardInput<'static> {
        CardInput {
            title: "A faster way to catch an itch",
            description: "Save a thought. Keep its context. Review it when the change is ready.",
            project: "DumbManager Rust",
            status: "ready",
            seed: "test",
        }
    }
    #[test]
    fn loop_is_periodic_and_text_is_still() {
        let a = render_frame(&input(), 0.0);
        let b = render_frame(&input(), TAU);
        let c = render_frame(&input(), 0.9);
        assert_eq!(a, b);
        assert_ne!(a, c);
        for y in 0..HEIGHT as usize {
            assert_eq!(
                &a[y * WIDTH as usize..y * WIDTH as usize + 700],
                &c[y * WIDTH as usize..y * WIDTH as usize + 700]
            );
        }
    }
    #[test]
    fn exports_decode_with_real_frames_and_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let out = export_card(dir.path(), &input(), "gif").unwrap();
        let mut decoder = gif::DecodeOptions::new()
            .read_info(std::fs::File::open(out.path).unwrap())
            .unwrap();
        assert_eq!((decoder.width(), decoder.height()), (WIDTH, HEIGHT));
        let mut count = 0;
        while decoder.read_next_frame().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, FRAMES);
        let out = export_card(dir.path(), &input(), "png").unwrap();
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(out.path).unwrap(),
        ));
        let info = decoder.read_info().unwrap();
        assert_eq!((info.info().width, info.info().height), (2400, 1350));
    }
    #[test]
    fn invalid_input_does_not_create_output() {
        let dir = tempfile::tempdir().unwrap();
        let bad = CardInput {
            title: "",
            ..input()
        };
        assert!(export_card(dir.path(), &bad, "gif").is_err());
        assert!(export_card(dir.path(), &input(), "../../other").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn word_wrap_stays_within_columns() {
        let lines = wrap("one extraordinarilylongwordwithnospace cafe\u{e9}", 7);
        assert!(lines.iter().all(|v| v.chars().count() <= 7));
        assert_eq!(
            lines.join("").replace(' ', ""),
            "oneextraordinarilylongwordwithnospacecafe\u{e9}"
        );
    }
}
