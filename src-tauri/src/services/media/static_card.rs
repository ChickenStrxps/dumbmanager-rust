//! Faithful static SVG card from DumbAPI manager_social.card (no source image by default).
use super::*;
fn copy_lines(
    value: &str,
    top: i32,
    height: usize,
    initial: usize,
    color: &str,
    name: &str,
) -> String {
    let mut wrapped = Vec::new();
    let mut font = initial;
    for f in (6..=initial).rev() {
        font = f;
        wrapped =
            dumbmanager_card_renderer::card_lines(value, (640.0 / (f as f64 * 0.65)) as usize);
        if wrapped.len() * (f + 7) <= height {
            break;
        }
    }
    let mut result = format!(
        "<g data-copy=\"{name}\" font-weight=\"{}\">",
        if name == "headline" { 700 } else { 400 }
    );
    for (i, line) in wrapped.iter().enumerate() {
        result.push_str(&format!(
            "<text x=\"64\" y=\"{}\" font-size=\"{font}\" fill=\"{color}\">{}</text>",
            top + font as i32 + i as i32 * (font as i32 + 7),
            xml(line)
        ));
    }
    result.push_str("</g>");
    result
}
pub(super) fn card(row: &Value, image_uri: &str) -> String {
    let id = row["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| row["headline"].as_str().unwrap_or(""));
    let digest = Sha256::digest(id.as_bytes());
    let seed = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let accents = [
        ("#ff5ccc", "#8b8fff"),
        ("#69f5ce", "#a6a0ff"),
        ("#ffc166", "#ff74b8"),
        ("#b9a1ff", "#65e5f4"),
    ];
    let (accent, secondary) = accents[(seed % 4) as usize];
    let creatures: [(&str, &[&str]); 6] = [
        (
            "cat",
            &[" /\\_/\\", "( o.o )", " > ^ <", " /| |\\", "(_| |_)"],
        ),
        (
            "crab",
            &[
                " \\     /",
                "  (o o)",
                " /|___|\\",
                "/ /   \\ \\",
                "v v   v v",
            ],
        ),
        (
            "fish",
            &[
                "       /\\",
                "  ____/  \\__",
                " /  o        >",
                "<     ____ /",
                " \\__/    \\/",
            ],
        ),
        (
            "ghost",
            &[
                "  .----.",
                " / o  o \\",
                "|   __   |",
                "|        |",
                " \\_/\\_/\\_/",
            ],
        ),
        ("frog", &["  @..@", " (----)", "( >__< )", "^^ ~~ ^^"]),
        (
            "moth",
            &[" \\  /", "/\\oo/\\", "> |<>| <", "\\/||\\/", "  /  \\"],
        ),
    ];
    let (creature, art) = creatures[((seed / 4) % 6) as usize];
    let (x, y, size) = if image_uri.is_empty() {
        (943, 238, 34)
    } else {
        (1032, 500, 14)
    };
    let mut critter=format!("<g data-critter=\"{creature}\" xml:space=\"preserve\" text-anchor=\"middle\" font-weight=\"700\">");
    for (i, line) in art.iter().enumerate() {
        critter.push_str(&format!(
            "<text x=\"{x}\" y=\"{}\" font-size=\"{size}\" fill=\"{}\">{}</text>",
            y + i * (size + 3),
            if i % 2 == 0 { accent } else { secondary },
            xml(line)
        ));
    }
    critter.push_str("</g>");
    let mut specks = String::new();
    for (x, y, size, opacity) in [
        (780, 138, 6, "0.8"),
        (1120, 152, 10, "0.7"),
        (738, 495, 5, "0.6"),
        (1102, 535, 5, "0.5"),
        (822, 537, 7, "0.7"),
        (1090, 106, 4, "0.8"),
    ] {
        specks.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{size}\" height=\"{size}\" fill=\"{secondary}\" opacity=\"{opacity}\"/>"));
    }
    let panel = if image_uri.is_empty() {
        format!("<rect x=\"762\" y=\"172\" width=\"370\" height=\"318\" fill=\"#12131f\" stroke=\"{secondary}\" stroke-opacity=\".45\"/><path d=\"M782 192h36m-36 0v36M1112 470h-36m36 0v-36\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"3\"/><text x=\"947\" y=\"459\" text-anchor=\"middle\" font-size=\"14\" letter-spacing=\"3\" fill=\"{secondary}\">[ BUILD LOG ]</text>")
    } else {
        format!("<rect x=\"752\" y=\"183\" width=\"388\" height=\"298\" fill=\"{accent}\" opacity=\".25\"/><rect x=\"744\" y=\"175\" width=\"388\" height=\"298\" fill=\"#12131f\" stroke=\"{secondary}\" stroke-width=\"2\"/><rect x=\"756\" y=\"187\" width=\"364\" height=\"274\" fill=\"#080910\"/><image x=\"756\" y=\"187\" width=\"364\" height=\"274\" preserveAspectRatio=\"xMidYMid meet\" href=\"{}\"/><text x=\"746\" y=\"156\" font-size=\"14\" letter-spacing=\"2\" fill=\"{secondary}\">ON SCREEN</text>",xml(image_uri))
    };
    let label = match row["stage"].as_str() {
        Some("accepted") => "WE\u{2019}RE SO BACK",
        Some("review") => "READY TO TEST",
        _ => "WORK IN PROGRESS",
    };
    let headline = copy_lines(
        row["headline"].as_str().unwrap_or(""),
        175,
        140,
        42,
        "#fcfaff",
        "headline",
    );
    let summary = copy_lines(
        row["summary"].as_str().unwrap_or(""),
        342,
        140,
        24,
        "#d7d7e8",
        "summary",
    );
    let button_width = label.chars().count() * 12 + 64;
    format!(concat!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2400\" height=\"1260\" viewBox=\"0 0 1200 630\">",
      "<defs><radialGradient id=\"glow\" cx=\"90%\" cy=\"5%\" r=\"100%\"><stop stop-color=\"{secondary}\" stop-opacity=\".23\"/><stop offset=\"1\" stop-color=\"#090a12\" stop-opacity=\"0\"/></radialGradient>",
      "<pattern id=\"grid\" width=\"24\" height=\"24\" patternUnits=\"userSpaceOnUse\"><rect width=\"2\" height=\"2\" fill=\"#aaa9cf\" opacity=\".13\"/></pattern></defs>",
      "<rect width=\"1200\" height=\"630\" fill=\"#090a12\"/><rect width=\"1200\" height=\"630\" fill=\"url(#glow)\"/><rect x=\"722\" y=\"24\" width=\"454\" height=\"582\" fill=\"url(#grid)\"/>",
      "<path d=\"M24 122V24h98M1078 24h98v98M1176 508v98h-98M122 606H24v-98\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"4\"/>",
      "<rect x=\"64\" y=\"60\" width=\"32\" height=\"32\" fill=\"{accent}\"/><rect x=\"72\" y=\"68\" width=\"8\" height=\"8\" fill=\"#090a12\"/><rect x=\"80\" y=\"76\" width=\"8\" height=\"8\" fill=\"#090a12\"/>",
      "<g font-family=\"monospace\"><text x=\"110\" y=\"82\" font-size=\"19\" font-weight=\"700\" letter-spacing=\"2\" fill=\"#f9f6ff\">DUMBMANAGER</text>",
      "<text x=\"1134\" y=\"81\" text-anchor=\"end\" font-size=\"15\" letter-spacing=\"2\" fill=\"{secondary}\">DEV / LOG</text><path d=\"M64 120H1136\" stroke=\"{secondary}\" stroke-opacity=\".3\"/>",
      "<text x=\"64\" y=\"154\" font-size=\"14\" letter-spacing=\"3\" fill=\"{accent}\">WHAT CHANGED</text>{headline}",
      "<rect x=\"64\" y=\"325\" width=\"60\" height=\"4\" fill=\"{accent}\"/><rect x=\"130\" y=\"325\" width=\"10\" height=\"4\" fill=\"{secondary}\"/>{summary}{panel}{critter}{specks}",
      "<rect x=\"64\" y=\"532\" width=\"{button_width}\" height=\"40\" fill=\"{accent}\"/><text x=\"82\" y=\"558\" font-size=\"18\" font-weight=\"700\" fill=\"#090a12\">[+] {label}</text>",
      "<path d=\"M64 590H1136\" stroke=\"{secondary}\" stroke-opacity=\".25\"/></g></svg>"),secondary=secondary,accent=accent,headline=headline,summary=summary,panel=panel,critter=critter,specks=specks,button_width=button_width,label=label)
}
