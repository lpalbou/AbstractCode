//! The transcript image pipeline, end to end on real encoded bytes.
//!
//! Every picture the TUI shows — a tool's image artifact, and the echo
//! preview of an image the user hands `/attach` — travels one road:
//!
//!   gateway bytes
//!     -> `abstracttui::gfx::decode_image`      (runner::fetch_image)
//!     -> `runner::downscale_for_transcript`    (worker thread, F3 ceiling)
//!     -> `gfx::mosaic::render_to_cells`        (ui::transcript_view::image_block)
//!
//! A failure anywhere on it is not a blank space: `render_item`'s
//! `Item::Image` arm prints the decoder's own message, in the error
//! color, in the card where the picture belonged.
//!
//! The road's decode end used to reject PROGRESSIVE JPEG, which is what
//! phone cameras, image editors and "save for web" emit by default — so
//! an ordinary photo dropped on the composer rendered as
//! `image decode failed: parse: jpeg: progressive JPEG not supported
//! (baseline only)`. abstracttui 0.3.5 taught the decoder SOF2 frames;
//! this file is the floor under that, and under the rest of the road.
//!
//! Fixtures are base64 so the repo keeps its all-text fixture set. Both
//! encode the SAME 96x64 picture — smooth sinusoidal color gradients
//! (real AC energy, unlike a flat swatch) plus a yellow ellipse and a
//! blue square. Regenerate with PIL:
//!
//! ```text
//! im.save("prog.jpg", "JPEG", quality=82, progressive=True, optimize=True)
//! im.save("ref.png",  "PNG")
//! ```

use abstracttui::base::Rect;
use abstracttui::gfx::{base64, decode_image, mosaic};
use abstracttui::term::Capabilities;
use abstracttui::widgets::Bitmap;

const PROGRESSIVE_JPEG_B64: &str = include_str!("fixtures/progressive_jpeg.b64");
const REFERENCE_PNG_B64: &str = include_str!("fixtures/progressive_jpeg_ref_png.b64");

fn fixture(b64: &str) -> Vec<u8> {
    let stripped: String = b64.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::decode(&stripped).expect("fixture is valid base64")
}

/// Mean-squared-error PSNR in dB over RGB. `INFINITY` for identical
/// images; a decoder emitting garbage for the right dimensions scores
/// in the single digits, so this separates "decoded" from "decoded the
/// actual picture" — which a dimensions-only assert cannot.
fn psnr(a: &Bitmap, b: &Bitmap) -> f64 {
    assert_eq!(
        (a.width(), a.height()),
        (b.width(), b.height()),
        "PSNR needs matching dimensions"
    );
    let mut se = 0f64;
    let mut n = 0f64;
    for y in 0..a.height() {
        for x in 0..a.width() {
            let (p, q) = (a.get(x, y).unwrap(), b.get(x, y).unwrap());
            for (u, v) in [(p.r, q.r), (p.g, q.g), (p.b, q.b)] {
                let d = u as f64 - v as f64;
                se += d * d;
                n += 1.0;
            }
        }
    }
    if se == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0 * 255.0 / (se / n)).log10()
}

/// The fixture really is progressive — a SOF2 frame, not SOF0. Without
/// this the suite could keep passing after someone "regenerates" the
/// fixture as a baseline JPEG, and the floor below would guard nothing.
#[test]
fn progressive_fixture_carries_a_sof2_frame() {
    let bytes = fixture(PROGRESSIVE_JPEG_B64);
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "not a JPEG (missing SOI)");
    let mut i = 2;
    let mut sof = None;
    while i + 4 <= bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            sof = Some(marker);
            break;
        }
        if marker == 0xDA {
            break;
        }
        i += 2 + u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
    }
    assert_eq!(
        sof,
        Some(0xC2),
        "fixture must be a PROGRESSIVE JPEG (SOF2); 0xC0 is baseline and would \
         make `progressive_jpeg_decodes_to_the_real_picture` vacuous"
    );
}

/// The decode end of the road: a progressive JPEG decodes, and decodes
/// to the picture the PNG holds — not to noise of the right size.
#[test]
fn progressive_jpeg_decodes_to_the_real_picture() {
    let decoded = decode_image(&fixture(PROGRESSIVE_JPEG_B64)).unwrap_or_else(|e| {
        panic!(
            "progressive JPEG must decode (abstracttui >= 0.3.5); the TUI prints \
             this message in the image card where the picture belongs: {e}"
        )
    });
    assert_eq!((decoded.width(), decoded.height()), (96, 64));

    let reference = decode_image(&fixture(REFERENCE_PNG_B64)).expect("reference PNG decodes");
    let db = psnr(&decoded, &reference);
    // Quality-82 JPEG against its lossless source measures ~24.8 dB on
    // this picture. 18 dB leaves room for encoder/decoder rounding
    // while staying far above the single digits a garbled decode scores.
    assert!(
        db >= 18.0,
        "progressive decode is not the reference picture: {db:.1} dB PSNR"
    );
}

/// The whole road, at the geometry `ui::transcript_view::image_block`
/// asks for: decode -> transcript downscale -> mosaic cells.
#[test]
fn progressive_jpeg_reaches_the_transcript_as_painted_cells() {
    let decoded = decode_image(&fixture(PROGRESSIVE_JPEG_B64)).expect("progressive JPEG decodes");
    let bounded = abstractcode::runner::downscale_for_transcript(decoded);
    // Already inside the F3 ceiling, so the pre-scale is a no-op here —
    // the point is that the transcript's own step accepts the bitmap.
    assert!(bounded.width() <= 1024 && bounded.height() <= 168);

    // 24x14 = the in-feed block's widest ladder at IMAGE_ROWS rows.
    let cells =
        mosaic::render_to_cells(&bounded, Rect::new(0, 0, 24, 14), &Capabilities::default());
    assert_eq!(
        cells.len(),
        24 * 14,
        "one patch per cell of the target rect"
    );
    let painted = cells.iter().filter(|c| c.ch != ' ').count();
    assert!(
        painted > 24 * 14 / 2,
        "mosaic produced {painted} painted cells — the picture did not reach the feed"
    );
}

/// The in-feed picture follows the terminal's proved capabilities.
/// `image_block` reads `app::current_caps()` at DRAW time precisely so
/// a probe upgrade sharpens the next repaint; this pins the two ends of
/// that ladder so a future engine bump that pins one family (the bug
/// abstracttui 0.3.6 fixed in `widgets::Image`) is caught here.
#[test]
fn mosaic_density_follows_terminal_capabilities() {
    let img = decode_image(&fixture(REFERENCE_PNG_B64)).expect("reference PNG decodes");
    let rect = Rect::new(0, 0, 24, 14);

    let conservative = Capabilities::default();
    assert_eq!(
        mosaic::MosaicMode::auto(&conservative).0,
        mosaic::MosaicMode::HalfBlock
    );

    let mut modern = Capabilities::default();
    modern.unicode_ok = true;
    modern.truecolor = true;
    assert_eq!(
        mosaic::MosaicMode::auto(&modern).0,
        mosaic::MosaicMode::Quadrant,
        "a unicode + truecolor terminal must get 2x2 subpixels per cell"
    );

    // Denser mode => a richer glyph alphabet in the actual output, not
    // just a different enum: half blocks draw from {' ', '▀'}, quadrants
    // from the sixteen quadrant glyphs.
    let alphabet = |caps: &Capabilities| {
        let mut cs: Vec<char> = mosaic::render_to_cells(&img, rect, caps)
            .iter()
            .map(|c| c.ch)
            .collect();
        cs.sort_unstable();
        cs.dedup();
        cs
    };
    let (half, quad) = (alphabet(&conservative), alphabet(&modern));
    assert!(
        quad.len() > half.len(),
        "quadrant output ({quad:?}) is no richer than half-block output ({half:?})"
    );
}
