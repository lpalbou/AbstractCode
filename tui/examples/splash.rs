//! splash — preview the boot animation without launching the client.
//!
//! Live (in a terminal):    `cargo run --example splash [--2d|--3d]`
//! Contact sheet (agents):  `cargo run --example splash -- --sheet <dir>`
//!
//! The sheet mode renders the storyboard's keyframes for BOTH lanes to
//! SVG plus an index.html — the same frames the beat tests assert on,
//! in a form a human (or an agent with a browser) can look at.

use abstractcode::ui::splash::{BootSplash, Lane, HARD_CUTOFF_MS, TOTAL_MS};
use abstracttui::base::{Rect, Size};
use abstracttui::boot::{play, SplashFrameSource, SplashOptions, SplashOutcome, TerminalIo};
use abstracttui::render::Screenshot;
use abstracttui::term::{Capabilities, EnterOptions, Terminal};
use abstracttui::theme;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let theme_id =
        std::env::var("ABSTRACTTUI_THEME").unwrap_or_else(|_| theme::DEFAULT_THEME_ID.into());
    let (theme, warning) = theme::resolve(&theme_id);
    if let Some(w) = warning {
        eprintln!("{w}");
    }
    if let Some(i) = args.iter().position(|a| a == "--lockup") {
        // The IDLE lockup at several positions of its light band: the
        // mark, the wordmark and the tagline as the first screen draws
        // them, so the sheen can be looked at instead of guessed at.
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        lockup_sheet(&dir, Size::new(env_num("SHEET_COLS", 100), 16), theme);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--sheet") {
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        let cols: i32 = env_num("SHEET_COLS", 100);
        let rows: i32 = env_num("SHEET_ROWS", 30);
        sheet(&dir, Size::new(cols, rows), theme);
        return;
    }

    let caps = Capabilities::detect_env();
    let lane = if args.iter().any(|a| a == "--2d") {
        Lane::Cells
    } else if args.iter().any(|a| a == "--3d") {
        Lane::Depth
    } else {
        Lane::for_caps(&caps)
    };
    if let Err(reason) = abstracttui::boot::should_splash(&caps) {
        println!("splash: skipped — {reason}");
        return;
    }
    let mut term = match abstracttui::term::UnixTerminal::new() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("splash: no terminal: {e:?}");
            return;
        }
    };
    if let Err(e) = term.enter(&EnterOptions::default()) {
        eprintln!("splash: could not enter raw mode: {e:?}");
        return;
    }
    let mut source = BootSplash::new(lane);
    let mut io = TerminalIo::new(&mut term);
    let present = abstracttui::boot::player::splash_present_caps(&caps);
    let t0 = std::time::Instant::now();
    let mut clock = move || t0.elapsed().as_millis() as u64;
    let opts = SplashOptions {
        fps: 30,
        total_ms: TOTAL_MS,
        hard_cutoff_ms: HARD_CUTOFF_MS,
        ..SplashOptions::default()
    };
    let outcome = play(&mut io, &mut source, theme, &present, &opts, &mut clock);
    let _ = io.finish();
    let _ = term.leave();
    match outcome {
        Ok(SplashOutcome::Completed) => println!("splash[{lane:?}]: completed"),
        Ok(other) => println!("splash[{lane:?}]: {other:?}"),
        Err(e) => eprintln!("splash[{lane:?}]: {e:?}"),
    }
}

fn env_num(key: &str, default: i32) -> i32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Keyframes: one per storyboard beat, plus a fine sweep of the arrival
/// and of the hold-then-fade handoff at the end.
const KEYFRAMES_MS: [u32; 16] = [
    0, 150, 320, 520, 720, 900, 1000, 1180, 1320, 1500, 1700, 1900, 2200, 2400, 2550, 2700,
];

fn sheet(dir: &str, size: Size, theme: &'static abstracttui::theme::Theme) {
    let _ = std::fs::create_dir_all(dir);
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>boot animation</title>\
         <style>body{background:#111;color:#ddd;font:13px system-ui;margin:24px}\
         h2{font-weight:600;margin:24px 0 8px}\
         .row{display:flex;flex-wrap:wrap;gap:10px}\
         figure{margin:0}figcaption{opacity:.6;font-size:11px;margin-top:2px}\
         .shot{width:460px;border:1px solid #333}.shot svg{width:100%;height:auto;display:block}</style>",
    );
    for (label, lane) in [("depth (3D)", Lane::Depth), ("cells (2D)", Lane::Cells)] {
        html.push_str(&format!("<h2>{label}</h2><div class=row>"));
        // ONE source per lane, stepped forward: trails and sparks are
        // history-bearing, exactly as in playback.
        let mut source = BootSplash::new(lane);
        let mut prev = 0u32;
        for ms in KEYFRAMES_MS {
            // Walk at 30 fps so the afterglow/particles see real steps.
            let mut t = prev;
            while t + 33 < ms {
                t += 33;
                source.render(t as f32 / 1000.0, size, theme);
            }
            let frame = source.render(ms as f32 / 1000.0, size, theme);
            let name = format!("{}-{ms:04}.svg", lane_slug(lane));
            let svg = Screenshot::from_surface(frame).to_svg();
            let _ = std::fs::write(std::path::Path::new(dir).join(&name), &svg);
            // Inline (not <img src>): the sheet must render from a
            // single self-contained file, wherever it is opened.
            html.push_str(&format!(
                "<figure><div class=shot>{svg}</div><figcaption>{ms} ms</figcaption></figure>"
            ));
            prev = ms;
        }
        html.push_str("</div>");
    }
    let index = std::path::Path::new(dir).join("index.html");
    let _ = std::fs::write(&index, html);
    println!("sheet: {}", index.display());
}

fn lane_slug(lane: Lane) -> &'static str {
    match lane {
        Lane::Depth => "depth",
        Lane::Cells => "cells",
    }
}

/// Render the idle lockup (hero mark + wordmark + tagline) at a spread
/// of sheen positions. Mirrors `ui::logo::hero`'s composition, through
/// the same public light functions, so what this shows is what the app
/// draws.
fn lockup_sheet(dir: &str, size: Size, theme: &'static abstracttui::theme::Theme) {
    use abstractcode::ui::logo;
    use abstracttui::render::{Cell, Glyph, Style, Surface};
    let _ = std::fs::create_dir_all(dir);
    let t = &theme.tokens;
    let mark_rows = logo::hero_rows(45).unwrap_or(9);
    let word_w = abstracttui::text::width(logo::WORD_TOP);
    let (hi, _) = logo::floored_highlights(t);
    let lum = |c: abstracttui::base::Rgba| {
        0.2126 * (c.r as f64 / 255.0)
            + 0.7152 * (c.g as f64 / 255.0)
            + 0.0722 * (c.b as f64 / 255.0)
    };
    let ratio = (lum(hi).max(lum(t.text_muted)) + 0.05) / (lum(hi).min(lum(t.text_muted)) + 0.05);
    println!(
        "theme {}: muted {:?} -> shimmer {:?} = {:.2}:1",
        theme.id, t.text_muted, hi, ratio
    );
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>lockup</title>\
         <style>body{background:#0b0b0b;color:#bbb;font:11px system-ui;margin:8px}\
         figure{margin:0 0 8px}figcaption{opacity:.6}\
         .shot{border:1px solid #2a2a2a}.shot svg{width:100%;height:auto;display:block}</style>",
    );
    for frame in [0u64, 20, 34, 44, 52, 60, 70] {
        let mut s = Surface::new(size, Cell::EMPTY);
        let rect = Rect::new(0, 0, size.w, size.h);
        s.fill_rect(rect, Cell::new(Glyph::SPACE).with_fg(t.bg).with_bg(t.bg));
        let x0 = (size.w - word_w) / 2;
        let pos = logo::sheen_pos(frame, word_w);
        let light = |x: i32, y: i32| logo::sheen_weight(x - x0, y - 1, pos);
        let mark_w = (mark_rows as f32 * 2.0 * 0.75) as i32;
        let mark = Rect::new((size.w - mark_w) / 2, 1, mark_w, mark_rows);
        abstractcode::ui::splash::draw_settled_mark(
            &mut s,
            mark,
            t.bg,
            1.0,
            logo::pulse_weight(frame),
            &light,
        );
        for (r, art) in [logo::WORD_TOP, logo::WORD_BOT].into_iter().enumerate() {
            let y = 1 + mark_rows + r as i32;
            for (col, ch) in art.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let x = x0 + col as i32;
                let ink = logo::lerp_ink(t.text_muted, hi, light(x, y));
                s.draw_text(x, y, &ch.to_string(), Style::new().fg(ink));
            }
        }
        let tag = "0.4.0 · rendered by AbstractTUI";
        s.draw_text(
            (size.w - tag.chars().count() as i32) / 2,
            3 + mark_rows,
            tag,
            Style::new().fg(t.text_faint),
        );
        let svg = Screenshot::from_surface(&s).to_svg();
        let _ = std::fs::write(
            std::path::Path::new(dir).join(format!("lockup-{frame:03}.svg")),
            &svg,
        );
        html.push_str(&format!(
            "<figure><div class=shot>{svg}</div><figcaption>frame {frame}</figcaption></figure>"
        ));
    }
    let index = std::path::Path::new(dir).join("index.html");
    let _ = std::fs::write(&index, html);
    println!("lockup: {}", index.display());
}
