//! splash — preview the boot animation without launching the client.
//!
//! Live (in a terminal):    `cargo run --example splash [--2d|--3d]`
//! Contact sheet (agents):  `cargo run --example splash -- --sheet <dir>`
//!
//! The sheet mode renders the storyboard's keyframes for BOTH lanes to
//! SVG plus an index.html — the same frames the beat tests assert on,
//! in a form a human (or an agent with a browser) can look at.

use abstractcode_tui::ui::splash::{BootSplash, Lane, HARD_CUTOFF_MS, TOTAL_MS};
use abstracttui::base::Size;
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

/// Keyframes: one per storyboard beat, plus a fine sweep of the arrival.
const KEYFRAMES_MS: [u32; 12] = [
    0, 150, 320, 520, 720, 900, 1000, 1180, 1320, 1500, 1700, 1900,
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
