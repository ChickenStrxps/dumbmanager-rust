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

// Bundled, licensed fonts; no system-font discovery or replacement glyphs.
fn fonts() -> &'static [fontdue::Font] {
    static FONTS: std::sync::OnceLock<Vec<fontdue::Font>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        [
            include_bytes!("../../../web/fonts/IBMPlexMono.ttf").as_slice(),
            include_bytes!("../../../web/fonts/SpaceGrotesk.ttf").as_slice(),
            include_bytes!("../../../web/fonts/Silkscreen.ttf").as_slice(),
        ]
        .iter()
        .map(|bytes| {
            fontdue::Font::from_bytes(*bytes, fontdue::FontSettings::default())
                .expect("bundled font")
        })
        .collect()
    })
}
fn font_for(ch: char) -> Option<&'static fontdue::Font> {
    fonts().iter().find(|font| font.lookup_glyph_index(ch) != 0)
}
fn validate_text(input: &CardInput<'_>) -> io::Result<()> {
    for ch in input
        .title
        .chars()
        .chain(input.description.chars())
        .chain(input.project.chars())
    {
        if !ch.is_whitespace() && font_for(ch).is_none() {
            return Err(error(format!(
                "Bundled fonts cannot render U+{:04X} faithfully.",
                ch as u32
            )));
        }
    }
    Ok(())
}
impl Canvas {
    fn font_text(&mut self, x: i32, y: i32, text: &str, size: f32, color: u8) {
        let mut pen = x as f32;
        for ch in text.chars() {
            if ch.is_whitespace() {
                pen += size * 0.61;
                continue;
            }
            if let Some(font) = font_for(ch) {
                let (metrics, bitmap) = font.rasterize(ch, size);
                let top = y + size as i32 - metrics.height as i32 - metrics.ymin;
                for yy in 0..metrics.height {
                    for xx in 0..metrics.width {
                        if bitmap[yy * metrics.width + xx] >= 96 {
                            self.rect(
                                pen as i32 + metrics.xmin + xx as i32,
                                top + yy as i32,
                                1,
                                1,
                                color,
                            );
                        }
                    }
                }
                pen += metrics.advance_width;
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

/// Static SVG copy wrapping follows the original card's display-column rules.
pub fn card_lines(value: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let paragraphs: Vec<&str> = value.lines().collect();
    let paragraphs = if paragraphs.len() > 6 {
        vec![paragraphs.join(" ")]
    } else {
        paragraphs.into_iter().map(str::to_owned).collect()
    };
    let weight = |s: &str| {
        s.chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum::<usize>()
    };
    let mut result = Vec::new();
    for paragraph in paragraphs {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty()
                && weight(&format!("{line} {word}")) > width
                && weight(word) <= width
            {
                result.push(std::mem::take(&mut line));
            }
            let pending = format!("{}{word}", if line.is_empty() { "" } else { " " });
            for ch in pending.chars() {
                if weight(&line) + UnicodeWidthChar::width(ch).unwrap_or(0) > width {
                    result.push(line.trim_end().to_string());
                    line.clear();
                }
                if !line.is_empty() || !ch.is_whitespace() {
                    line.push(ch);
                }
            }
        }
        if !line.is_empty() {
            result.push(line);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
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
    let project = if input.project.trim().is_empty() {
        "UNSORTED"
    } else {
        input.project
    };
    let project_lines = wrap(project, 65);
    for (i, line) in project_lines.iter().take(2).enumerate() {
        c.font_text(52, 112 + i as i32 * 13, line, 11.0, 10);
    }
    // Fit all draft copy into the text column rather than silently dropping lines.
    let titles = wrap(input.title, 34);
    let title_size = (150.0 / titles.len().max(1) as f32).clamp(12.0, 25.0);
    let title_step = title_size * 1.25;
    for (i, line) in titles.iter().enumerate() {
        c.font_text(
            50,
            150 + (i as f32 * title_step) as i32,
            line,
            title_size,
            6,
        );
    }
    let start = 158 + (titles.len() as f32 * title_step) as i32;
    let description = wrap(input.description, 65);
    let available = (560 - start).max(1) as f32;
    let size = (available / (description.len().max(1) as f32 * 1.25)).min(15.0);
    for (i, line) in description.iter().enumerate() {
        c.font_text(52, start + (i as f32 * size * 1.25) as i32, line, size, 5);
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

/// Decode every frame before accepting a browser-rendered GIF into native storage.
#[derive(Debug)]
pub struct GifInfo {
    pub frames: usize,
    pub centiseconds: u64,
}
pub fn inspect_gif(data: &[u8]) -> io::Result<GifInfo> {
    if data.len() > 15 * 1024 * 1024 || !data.starts_with(b"GIF89a") || !data.ends_with(b";") {
        return Err(error("Invalid GIF."));
    }
    let mut decoder = gif::DecodeOptions::new()
        .read_info(std::io::Cursor::new(data))
        .map_err(error)?;
    if (decoder.width(), decoder.height()) != (WIDTH, HEIGHT) {
        return Err(error("Invalid card dimensions."));
    }
    let mut frames = 0usize;
    let mut centiseconds = 0u64;
    while let Some(frame) = decoder.read_next_frame().map_err(error)? {
        frames += 1;
        centiseconds += u64::from(frame.delay);
        if frames > 96
            || frame.width == 0
            || frame.height == 0
            || u32::from(frame.left) + u32::from(frame.width) > u32::from(WIDTH)
            || u32::from(frame.top) + u32::from(frame.height) > u32::from(HEIGHT)
            || centiseconds > 576
        {
            return Err(error("Invalid GIF frame bounds."));
        }
    }
    if frames == 0 || centiseconds == 0 || decoder.repeat() != gif::Repeat::Infinite {
        return Err(error("GIF must be a looping animation."));
    }
    Ok(GifInfo {
        frames,
        centiseconds,
    })
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
    validate_text(input)?;
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

pub mod sanitize;

/// Encode a bounded native RGBA capture without an external image tool.
pub fn png_rgba(width: u32, height: u32, rgba: &[u8]) -> io::Result<Vec<u8>> {
    let count = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || count > 16_777_216 || rgba.len() as u64 != count * 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid capture size",
        ));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(rgba)?;
    }
    Ok(out)
}
