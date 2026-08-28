//! Attachment preview: what a staged file ACTUALLY is, shown BEFORE it
//! rides a run.
//!
//! The engine can draw pictures (`gfx::decode_image` + the mosaic
//! ladder — the same pair the transcript's in-feed image block uses),
//! so a chip should never be a name and a byte count on trust alone:
//! `/attach preview` (and `p` in the manager) opens the real bytes.
//!
//! Loading is I/O + CPU (a 4000×3000 JPEG decode is not free), so it
//! runs on the WORKER — `Cmd::LoadPreview` spawns a named thread and
//! posts the body back through the wake queue, exactly like
//! `Runner::fetch_answer`. This module is the pure half: bytes in,
//! `PreviewBody` out, no signals and no terminal.
//!
//! Honesty rules, in the house style (ADR 0001 — no silent truncation):
//! - Text reads at most [`TEXT_PREVIEW_MAX_BYTES`]; when the file is
//!   bigger the body CARRIES that fact and the modal prints it. A
//!   preview that quietly shows half a file is a lie about the file.
//!   The same rule governs what we CHANGE to make the text readable:
//!   an ANSI-stripped log and a transcoded UTF-16 document both say so.
//! - The engine decodes PNG, JPEG and GIF (0.6.0 — an animated GIF
//!   previews as its first frame). WebP/BMP/TIFF attach and upload
//!   perfectly well, so the refusal names the format and says so —
//!   never "unsupported file", which reads as "your attachment is
//!   broken".
//! - Decoder errors pass through VERBATIM (`gfx::decode_image`'s
//!   messages are already named and prefixed). This is the second such
//!   site; `ui::transcript_view`'s image-block error line is the first.

use std::sync::Arc;

use abstracttui::widgets::Bitmap;

/// Text bytes read for a preview. The modal scrolls what it is given
/// and labels the cut; the cap exists so `/attach preview` on a 2 GB
/// log cannot buffer the log.
pub const TEXT_PREVIEW_MAX_BYTES: u64 = 512 * 1024;
/// Encoded-image bytes we will hand the decoder. Above this the wait
/// (and the transient RGBA allocation behind it) stops being a preview.
pub const IMAGE_DECODE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Decode-time pixel ceiling, contain-fit — the modal mosaic is at most
/// a few hundred cells (≤ 4 px/cell on the braille ladder), so this
/// carries generous headroom while a 108 MP phone photo still drops to
/// ≤ ~2 MB of retained RGBA. Same reasoning as
/// `runner::IMAGE_PX_CEILING`, one size class up because the modal is
/// bigger than the 14-row in-feed block.
pub const PREVIEW_PX_CEILING: (u32, u32) = (1024, 512);
/// Tabs expand here: `text::wrap` STRIPS control clusters by contract,
/// so a tab-indented source file would otherwise preview flush-left.
const TAB_WIDTH: usize = 4;
/// Bytes inspected for the binary/text decision.
const SNIFF_BYTES: usize = 8192;

/// One loaded preview body.
#[derive(Clone)]
pub enum PreviewBody {
    /// The loader thread is still working (what the modal opens with).
    Loading,
    Text(TextPreview),
    Image(ImagePreview),
    /// No preview is possible — `reason` is user-facing and names the
    /// format or the failure. Never a dead end: the file itself is
    /// still a perfectly good attachment.
    Unavailable {
        reason: String,
    },
}

#[derive(Clone)]
pub struct TextPreview {
    /// Logical lines (tabs expanded, `\r\n` and lone `\r` normalized).
    /// The modal wraps these at draw width; nothing is dropped here.
    pub lines: Vec<String>,
    /// Bytes actually read.
    pub shown_bytes: u64,
    /// Bytes the file holds.
    pub total_bytes: u64,
    /// `shown_bytes < total_bytes` — the modal SAYS so.
    pub truncated: bool,
    /// Invalid encoding was replaced (U+FFFD) — also labeled.
    pub lossy: bool,
    /// ANSI escape sequences were removed so the text reads (colored
    /// build logs). Labeled: the preview says the file has color codes
    /// it is not showing, rather than silently dropping them OR
    /// rendering `[32mok` garbage.
    pub ansi_stripped: bool,
    /// The source was UTF-16 (BOM-detected) and was transcoded.
    pub utf16: bool,
}

#[derive(Clone)]
pub struct ImagePreview {
    /// Already contain-fit to [`PREVIEW_PX_CEILING`] on the loader
    /// thread — the UI thread only ever sees a bounded bitmap.
    pub bitmap: Arc<Bitmap>,
    /// Pixel size BEFORE the preview downscale (what the file holds —
    /// the number the operator actually wants to know).
    pub source_px: (u32, u32),
    /// "PNG" / "JPEG".
    pub format: &'static str,
}

/// What the preview modal renders: the file's identity plus its body.
/// `seq` is the staleness guard — two quick previews must not let the
/// slower loader overwrite the newer file (the `upsert_image` lesson,
/// one lane over).
#[derive(Clone)]
pub struct PreviewState {
    pub seq: u64,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub body: PreviewBody,
}

impl PreviewState {
    /// The state a preview OPENS with, before the loader answers.
    pub fn loading(seq: u64, path: String, name: String, size: u64) -> Self {
        PreviewState {
            seq,
            path,
            name,
            size,
            body: PreviewBody::Loading,
        }
    }
}

/// Load one file's preview body. Runs on the loader thread; every
/// failure returns `Unavailable` with a reason a human can act on —
/// this function does not panic and never returns an empty body.
pub fn load(path: &str) -> PreviewBody {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return PreviewBody::Unavailable {
                reason: format!("cannot read this file: {e}"),
            }
        }
    };
    if meta.is_dir() {
        return PreviewBody::Unavailable {
            reason: "that's a folder — preview takes one file".into(),
        };
    }
    if !meta.is_file() {
        // Fifos/devices: a read would block the loader thread forever.
        return PreviewBody::Unavailable {
            reason: "not a regular file — nothing to preview".into(),
        };
    }
    let total = meta.len();
    if total == 0 {
        return PreviewBody::Unavailable {
            reason: "empty file (0 bytes)".into(),
        };
    }
    let head = match read_prefix(path, TEXT_PREVIEW_MAX_BYTES) {
        Ok(b) => b,
        Err(e) => {
            return PreviewBody::Unavailable {
                reason: format!("cannot read this file: {e}"),
            }
        }
    };
    if abstracttui::gfx::sniff_format(&head).is_some() {
        return load_image(path, total);
    }
    // UTF-16 BEFORE the binary heuristic: every other byte of ASCII
    // UTF-16 is NUL, so `looks_binary` would call a perfectly readable
    // document binary. Only a BOM'd file is claimed — BOM-less UTF-16
    // is genuinely undetectable and falls through honestly.
    if let Some(big_endian) = utf16_bom(&head) {
        return PreviewBody::Text(utf16_preview(head, total, big_endian));
    }
    if let Some(reason) = named_binary(&head) {
        return PreviewBody::Unavailable { reason };
    }
    if looks_binary(&head) {
        return PreviewBody::Unavailable {
            reason: "binary file — no text or image preview".into(),
        };
    }
    PreviewBody::Text(text_preview(head, total))
}

/// Decode an image file into a bounded bitmap. The MAGIC decided we are
/// here (`sniff_format`), so the extension is irrelevant — a `.png` that
/// holds JPEG bytes previews as the JPEG it is.
fn load_image(path: &str, total: u64) -> PreviewBody {
    if total > IMAGE_DECODE_MAX_BYTES {
        return PreviewBody::Unavailable {
            reason: format!(
                "{} image — too large to decode for preview (ceiling {})",
                crate::paths::human_size(total),
                crate::paths::human_size(IMAGE_DECODE_MAX_BYTES)
            ),
        };
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return PreviewBody::Unavailable {
                reason: format!("cannot read this file: {e}"),
            }
        }
    };
    let format = match abstracttui::gfx::sniff_format(&bytes) {
        Some(abstracttui::gfx::ImageFormat::Png) => "PNG",
        Some(abstracttui::gfx::ImageFormat::Jpeg) => "JPEG",
        // 0.6.0: GIF sniffs as an image (an animated one decodes below
        // as its first frame — the honest still for a preview pane).
        Some(abstracttui::gfx::ImageFormat::Gif) => "GIF",
        // `ImageFormat` is #[non_exhaustive] (0.4.0's own migration
        // note): a format this build has no name for still routes to
        // `decode_image` below; only the LABEL degrades to the generic
        // word, never the preview.
        Some(_) => "image",
        // The prefix sniffed as an image and the full read did not:
        // the file changed under us between the two reads.
        None => {
            return PreviewBody::Unavailable {
                reason: "the file changed while it was being read".into(),
            }
        }
    };
    match abstracttui::gfx::decode_image(&bytes) {
        Ok(bitmap) => {
            let source_px = (bitmap.width(), bitmap.height());
            PreviewBody::Image(ImagePreview {
                bitmap: Arc::new(downscale_for_preview(bitmap)),
                source_px,
                format,
            })
        }
        // Verbatim: the engine's decode errors are already named
        // ("jpeg: …", "png: …") and the operator can act on them.
        Err(e) => PreviewBody::Unavailable {
            reason: format!("{format} decode failed: {e}"),
        },
    }
}

/// Contain-fit within [`PREVIEW_PX_CEILING`]; never upscales.
pub fn downscale_for_preview(bitmap: Bitmap) -> Bitmap {
    let (w, h) = (bitmap.width(), bitmap.height());
    let (cw, ch) = PREVIEW_PX_CEILING;
    if w <= cw && h <= ch {
        return bitmap;
    }
    let scale = (cw as f64 / w.max(1) as f64).min(ch as f64 / h.max(1) as f64);
    let nw = ((w as f64 * scale).round() as u32).clamp(1, cw);
    let nh = ((h as f64 * scale).round() as u32).clamp(1, ch);
    bitmap.resize_bilinear(nw, nh)
}

/// Read at most `cap` bytes. Deliberately not `fs::read` — the whole
/// point of the text cap is to never buffer the whole file.
fn read_prefix(path: &str, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Build the text body from the bytes we read.
fn text_preview(mut bytes: Vec<u8>, total: u64) -> TextPreview {
    let truncated = total > bytes.len() as u64;
    if truncated {
        // The cut can land mid-codepoint; trim the incomplete trailing
        // sequence so the cap does not manufacture a U+FFFD and then
        // report the file as lossy because of our own knife.
        if let Err(e) = std::str::from_utf8(&bytes) {
            if e.error_len().is_none() {
                bytes.truncate(e.valid_up_to());
            }
        }
    }
    let shown_bytes = bytes.len() as u64;
    let (text, lossy) = match String::from_utf8(bytes) {
        Ok(s) => (s, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    let (lines, ansi_stripped) = split_lines(&text, truncated);
    TextPreview {
        lines,
        shown_bytes,
        total_bytes: total,
        truncated,
        lossy,
        ansi_stripped,
        utf16: false,
    }
}

/// Normalize, de-ANSI, expand tabs, split. Returns the lines and
/// whether any ANSI sequence was removed.
fn split_lines(text: &str, truncated: bool) -> (Vec<String>, bool) {
    // BOTH line endings: `\r\n` (Windows) and a lone `\r` (classic Mac,
    // and the carriage returns a progress bar writes into a build log).
    // Without the second, `text::wrap` strips the CR as a control
    // cluster and the whole file previews as ONE run-on line.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut ansi_stripped = false;
    let mut lines: Vec<String> = normalized
        .split('\n')
        .map(|line| {
            let (clean, stripped) = strip_ansi(line);
            ansi_stripped |= stripped;
            expand_tabs(&clean)
        })
        .collect();
    // A file that ENDS with a newline does not have a trailing empty
    // line — `split` says otherwise, and the header prints this count.
    // (Only when we read the whole file: a cut lands mid-content, and
    // there the last piece is real.)
    if !truncated && lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    (lines, ansi_stripped)
}

/// `Some(big_endian)` when these bytes open with a UTF-16 BOM.
fn utf16_bom(head: &[u8]) -> Option<bool> {
    match head {
        [0xFE, 0xFF, ..] => Some(true),
        [0xFF, 0xFE, ..] => Some(false),
        _ => None,
    }
}

/// Transcode a BOM'd UTF-16 document. The read cap can land between the
/// two bytes of a unit — drop that half rather than manufacture a
/// replacement character out of our own cut.
fn utf16_preview(bytes: Vec<u8>, total: u64, big_endian: bool) -> TextPreview {
    let shown_bytes = bytes.len() as u64;
    let body = &bytes[2..bytes.len() - (bytes.len() - 2) % 2];
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| {
            if big_endian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    let lossy = String::from_utf16(&units).is_err();
    let text = String::from_utf16_lossy(&units);
    let truncated = total > shown_bytes;
    let (lines, ansi_stripped) = split_lines(&text, truncated);
    TextPreview {
        lines,
        shown_bytes,
        total_bytes: total,
        truncated,
        lossy,
        ansi_stripped,
        utf16: true,
    }
}

/// Remove ANSI escape sequences (CSI colors, OSC strings) from one
/// line. `text::wrap` strips the ESC byte alone and leaves `[32mok`
/// on screen, which is neither the file nor readable; the caller
/// LABELS the removal in the header.
fn strip_ansi(line: &str) -> (String, bool) {
    if !line.contains('\u{1b}') {
        return (line.to_string(), false);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            // CSI: parameters, then one final byte in @..~.
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: runs to BEL or ST (ESC \).
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-character escape (or a stray ESC at end of line).
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    (out, true)
}

/// Expand tabs to the next [`TAB_WIDTH`] stop. `text::wrap` strips
/// control clusters by contract, so without this every tab-indented
/// file previews flush-left — a preview that misrepresents the file.
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let pad = TAB_WIDTH - (col % TAB_WIDTH);
            out.extend(std::iter::repeat_n(' ', pad));
            col += pad;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Name the common formats we cannot draw, so the refusal is about the
/// FORMAT and never about the attachment's validity. Magic bytes only —
/// containers lie, extensions lie more.
fn named_binary(head: &[u8]) -> Option<String> {
    let named = |what: &str| {
        Some(format!(
            "{what} — the preview draws PNG, JPEG and GIF; this file still attaches and uploads normally"
        ))
    };
    if head.starts_with(b"%PDF") {
        return Some(
            "PDF — no inline preview here; the gateway extracts its text server-side when this attaches"
                .into(),
        );
    }
    // No GIF arm: since 0.6.0 the engine DECODES GIF, so `sniff_format`
    // claims it upstream and this function can never see one.
    if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        return named("WebP image");
    }
    // `BM` is two printable letters — "BMW,320i,2021" is a CSV, not a
    // bitmap. Claim BMP only when the rest of the 14-byte header agrees:
    // reserved fields zero and a pixel offset at or past the header.
    if head.len() >= 14 && head.starts_with(b"BM") {
        let reserved = u32::from_le_bytes([head[6], head[7], head[8], head[9]]);
        let offset = u32::from_le_bytes([head[10], head[11], head[12], head[13]]);
        if reserved == 0 && (14..=(1 << 20)).contains(&offset) {
            return named("BMP image");
        }
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return named("TIFF image");
    }
    if head.starts_with(b"\x7fELF") || head.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        return Some("executable binary — nothing to preview".into());
    }
    if head.starts_with(b"PK\x03\x04") {
        return Some(
            "zip-based file (docx/xlsx/jar/zip) — no inline preview; it still attaches normally"
                .into(),
        );
    }
    None
}

/// The text/binary decision: a NUL byte, or a heavy run of other
/// control bytes, means these are not characters. (`\t\n\r` and the
/// ESC of an ANSI-colored log do not count against it.)
fn looks_binary(head: &[u8]) -> bool {
    let sample = &head[..head.len().min(SNIFF_BYTES)];
    if sample.contains(&0) {
        return true;
    }
    let odd = sample
        .iter()
        .filter(|b| **b < 0x09 || (**b > 0x0d && **b < 0x20 && **b != 0x1b))
        .count();
    odd * 100 > sample.len() * 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("acode-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p.display().to_string()
    }

    fn text_of(body: &PreviewBody) -> &TextPreview {
        match body {
            PreviewBody::Text(t) => t,
            PreviewBody::Image(_) => panic!("image, expected text"),
            PreviewBody::Loading => panic!("loading, expected text"),
            PreviewBody::Unavailable { reason } => panic!("unavailable ({reason}), expected text"),
        }
    }

    fn reason_of(body: &PreviewBody) -> String {
        match body {
            PreviewBody::Unavailable { reason } => reason.clone(),
            _ => panic!("expected an unavailable body"),
        }
    }

    #[test]
    fn text_files_preview_as_lines() {
        let p = tmp("doc.md", b"# Title\n\nbody line\n");
        let body = load(&p);
        let t = text_of(&body);
        assert_eq!(t.lines[0], "# Title");
        assert_eq!(t.lines[2], "body line");
        assert!(!t.truncated);
        assert!(!t.lossy);
        assert_eq!(t.total_bytes, 19);
    }

    #[test]
    fn a_trailing_newline_is_not_an_extra_line() {
        let p = tmp("count.txt", b"one\ntwo\nthree\n");
        assert_eq!(text_of(&load(&p)).lines.len(), 3);
        // …but a real blank last line still counts.
        let q = tmp("blank_tail.txt", b"one\ntwo\n\n");
        assert_eq!(text_of(&load(&q)).lines.len(), 3);
    }

    #[test]
    fn tabs_expand_so_indentation_survives_the_wrapper() {
        let p = tmp("tabs.rs", b"fn a() {\n\tlet x = 1;\n}\n");
        let t = text_of(&load(&p)).clone();
        assert_eq!(t.lines[1], "    let x = 1;");
    }

    #[test]
    fn oversize_text_is_cut_and_says_so() {
        let big = vec![b'a'; (TEXT_PREVIEW_MAX_BYTES + 4096) as usize];
        let p = tmp("big.log", &big);
        let t = text_of(&load(&p)).clone();
        assert!(t.truncated, "the cut must be recorded, never silent");
        assert_eq!(t.shown_bytes, TEXT_PREVIEW_MAX_BYTES);
        assert_eq!(t.total_bytes, TEXT_PREVIEW_MAX_BYTES + 4096);
    }

    #[test]
    fn a_multibyte_char_split_by_the_cap_never_reads_as_lossy() {
        // "é" (2 bytes) straddling the cap: the trailing half is
        // trimmed, so the preview is NOT reported as lossy.
        let mut bytes = vec![b'a'; (TEXT_PREVIEW_MAX_BYTES - 1) as usize];
        bytes.extend_from_slice("é".as_bytes());
        bytes.extend_from_slice(b"tail");
        let p = tmp("split.txt", &bytes);
        let t = text_of(&load(&p)).clone();
        assert!(t.truncated);
        assert!(!t.lossy, "our own cut must not be blamed on the file");
        assert_eq!(t.shown_bytes, TEXT_PREVIEW_MAX_BYTES - 1);
    }

    #[test]
    fn invalid_utf8_previews_lossily_and_says_so() {
        let p = tmp("latin1.txt", b"caf\xe9 noir\n");
        let t = text_of(&load(&p)).clone();
        assert!(t.lossy);
        assert!(t.lines[0].contains('\u{fffd}'));
    }

    #[test]
    fn png_files_decode_to_a_bounded_bitmap() {
        let img = abstracttui::widgets::Bitmap::from_fn(2048, 1024, |x, y| {
            abstracttui::base::Rgba::rgb((x % 256) as u8, (y % 256) as u8, 90)
        });
        let p = tmp("wide.png", &abstracttui::gfx::png_encode::encode(&img));
        match load(&p) {
            PreviewBody::Image(i) => {
                assert_eq!(i.format, "PNG");
                assert_eq!(i.source_px, (2048, 1024));
                assert!(i.bitmap.width() <= PREVIEW_PX_CEILING.0);
                assert!(i.bitmap.height() <= PREVIEW_PX_CEILING.1);
            }
            other => panic!("expected an image, got {}", reason_of(&other)),
        }
    }

    #[test]
    fn magic_beats_extension_in_both_directions() {
        // PNG bytes named .txt still preview as the picture they are.
        let img = abstracttui::widgets::Bitmap::from_fn(4, 4, |_, _| {
            abstracttui::base::Rgba::rgb(10, 20, 30)
        });
        let p = tmp("liar.txt", &abstracttui::gfx::png_encode::encode(&img));
        assert!(matches!(load(&p), PreviewBody::Image(_)));
        // …and text bytes named .png preview as text.
        let q = tmp("liar.png", b"not a picture\n");
        assert!(matches!(load(&q), PreviewBody::Text(_)));
    }

    #[test]
    fn formats_the_engine_cannot_draw_are_named_not_dismissed() {
        let pdf = tmp("paper.pdf", b"%PDF-1.7\n\x00\x00binary");
        assert!(reason_of(&load(&pdf)).contains("PDF"));
        let webp = tmp("shot.webp", b"RIFF\x24\x00\x00\x00WEBPVP8 ");
        let r = reason_of(&load(&webp));
        assert!(r.contains("WebP"), "{r}");
        assert!(r.contains("attaches"), "the attachment is still fine: {r}");
    }

    /// GIF crossed the fence in 0.6.0: it is a format the engine DRAWS
    /// now. A real GIF previews as an image labeled GIF; a corrupt one
    /// carries the decoder's own named error — never the old
    /// "cannot draw" refusal, which would now be a lie about the engine.
    #[test]
    fn gif_previews_as_an_image_since_0_6() {
        // The canonical 1×1 GIF89a (2-color palette, one clear pixel).
        let one_px: &[u8] = &[
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2C,
            0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
            0x3B,
        ];
        let p = tmp("dot.gif", one_px);
        match load(&p) {
            PreviewBody::Image(i) => {
                assert_eq!(i.format, "GIF");
                assert_eq!(i.source_px, (1, 1));
            }
            other => panic!("expected an image, got {}", reason_of(&other)),
        }
        // A GIF header with no image data: the decoder's named error
        // passes through verbatim, same contract as PNG/JPEG.
        let broken = tmp("anim.gif", b"GIF89a\x01\x00\x01\x00\x00\x00\x00;");
        let r = reason_of(&load(&broken));
        assert!(r.starts_with("GIF decode failed: "), "{r}");
    }

    #[test]
    fn a_csv_that_starts_with_bm_is_text_not_a_bitmap() {
        // "BM" is two printable letters: the magic must agree with the
        // rest of the BMP header before the preview claims a bitmap.
        let p = tmp("cars.csv", b"BMW,320i,2021\nAudi,A4,2020\nVolvo,V60,2019\n");
        let t = text_of(&load(&p)).clone();
        assert_eq!(t.lines[0], "BMW,320i,2021");
        // A REAL BMP header still reports as one.
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&1234u32.to_le_bytes()); // file size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // reserved
        bmp.extend_from_slice(&54u32.to_le_bytes()); // pixel offset
        bmp.extend_from_slice(&[0u8; 8]);
        let q = tmp("real.bmp", &bmp);
        assert!(reason_of(&load(&q)).contains("BMP"));
    }

    #[test]
    fn carriage_return_only_line_endings_still_split() {
        // Classic-Mac endings, and the CRs a progress bar writes into a
        // build log: without normalizing them the whole file previews
        // as one run-on line (`text::wrap` strips the CR as a control).
        let p = tmp("cr.txt", b"first line\rsecond line\rthird line\r");
        let t = text_of(&load(&p)).clone();
        assert_eq!(t.lines.len(), 3);
        assert_eq!(t.lines[1], "second line");
    }

    #[test]
    fn utf16_with_a_bom_reads_as_the_text_it_is() {
        for (bom, be) in [([0xFFu8, 0xFE], false), ([0xFE, 0xFF], true)] {
            let mut bytes = bom.to_vec();
            for u in "hello\nwörld".encode_utf16() {
                bytes.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() });
            }
            let p = tmp(if be { "be.txt" } else { "le.txt" }, &bytes);
            let t = text_of(&load(&p)).clone();
            assert!(t.utf16, "the transcode is recorded");
            assert_eq!(t.lines, vec!["hello".to_string(), "wörld".to_string()]);
            assert!(!t.lossy);
        }
    }

    #[test]
    fn ansi_sequences_are_removed_and_the_removal_is_recorded() {
        let p = tmp(
            "build.log",
            b"\x1b[32mok\x1b[0m first entry\n\x1b]0;title\x07second entry\n",
        );
        let t = text_of(&load(&p)).clone();
        assert!(t.ansi_stripped, "the header must be able to say so");
        assert_eq!(t.lines[0], "ok first entry");
        assert_eq!(t.lines[1], "second entry");
        // A file with no escapes never claims a removal.
        let q = tmp("plain2.log", b"ok first entry\n");
        assert!(!text_of(&load(&q)).ansi_stripped);
    }

    #[test]
    fn broken_image_bytes_carry_the_decoder_message() {
        // A PNG signature with nothing behind it: the engine's own
        // named error reaches the operator.
        let p = tmp("truncated.png", b"\x89PNG\r\n\x1a\nrubbish");
        let r = reason_of(&load(&p));
        assert!(r.starts_with("PNG decode failed: "), "{r}");
    }

    #[test]
    fn unreadable_and_empty_paths_refuse_with_a_reason() {
        assert!(reason_of(&load("/definitely/not/here")).contains("cannot read"));
        let empty = tmp("empty.txt", b"");
        assert!(reason_of(&load(&empty)).contains("empty"));
        let dir = std::env::temp_dir().join(format!("acode-preview-{}", std::process::id()));
        assert!(reason_of(&load(&dir.display().to_string())).contains("folder"));
    }

    #[test]
    fn binary_without_a_known_magic_still_refuses_cleanly() {
        let p = tmp("blob.bin", &[0u8, 1, 2, 3, 255, 254, 0, 7]);
        assert!(reason_of(&load(&p)).contains("binary"));
    }

    #[test]
    fn an_ansi_colored_log_is_still_text() {
        let mut bytes = Vec::new();
        for _ in 0..40 {
            bytes.extend_from_slice(b"\x1b[32mok\x1b[0m line of log output\n");
        }
        let p = tmp("colored.log", &bytes);
        assert!(matches!(load(&p), PreviewBody::Text(_)));
    }
}
