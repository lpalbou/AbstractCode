//! The boot animation: AbstractCode's ~1.9 s launch identity.
//!
//! ## The idea
//!
//! Three luminous planes fly in from off-stage, overshoot, and lock into
//! the ascending **A** of the Abstract house mark — the layered
//! architecture drawn as the family's initial (`abstractframework.ai`).
//! The impact throws sparks, an accent rule ignites across the stage, and
//! the `ABSTRACT CODE` wordmark resolves under it in the SAME half-block
//! letterforms the idle screen carries (`ui::logo`), so the splash does
//! not cut to the app — it *becomes* it.
//!
//! ## Two lanes, one storyboard
//!
//! - [`Lane::Depth`] (truecolor): the mark as three SLABS in a real
//!   scene — the engine's software 3D pipeline (perspective, z-buffer,
//!   lambert shading, depth fog) with a camera that starts angled, so
//!   the planes read as LAYERS before they lock into the letterform.
//! - [`Lane::Cells`] (everything else): the same three planes as thick
//!   segments rasterized with coverage antialiasing. No 3D, no truecolor
//!   requirement, same silhouette and same gradient.
//!
//! Both render into a half-block bitmap at 2× and box-downsample, both
//! carry the motion afterglow, and both hand the lockup (wordmark, rule,
//! tagline, footer, chrome) to the SAME code — the typography cannot
//! drift between them, and the beats are one set of constants.
//!
//! Both lanes are `SplashFrameSource`s: the engine's splash player owns
//! pacing (wall-clock, frame-dropping), the skip check between every
//! frame, the exit fade and the hard cutoff. This module only answers
//! "what does frame `t` look like" — a pure function of (t, size, theme),
//! which is what makes the beats testable.
//!
//! ## Ink policy
//!
//! Text is THEME ink (it must sit on the user's palette); the mark is the
//! HOUSE ramp (`boot::identity::BRAND_RAMP` — the brand is the brand on
//! every theme). Nothing here invents color arithmetic: mixing goes
//! through `theme::derive::mix`.

use abstracttui::anim::particles::{Burst, ParticleField};
use abstracttui::anim::Easing;
use abstracttui::base::{Rect, Rgba, Size};
use abstracttui::boot::identity::{brand_ramp, BRAND_FIELD};
use abstracttui::boot::{play, SplashFrameSource, SplashOptions, SplashOutcome, TerminalIo};
use abstracttui::gfx::{mosaic, Bitmap, MosaicCell, MosaicMode};
use abstracttui::render::{Cell, Glyph, Style, Surface};
use abstracttui::term::{Capabilities, EnterOptions, KittyFlags, MouseMode};
use abstracttui::text;
use abstracttui::theme::derive::mix;
use abstracttui::theme::Theme;
use abstracttui::three::primitives::cuboid;
use abstracttui::three::texture::srgb8_to_linear;
use abstracttui::three::{
    Camera, Framebuffer, Light, Mat4, MaterialData, MeshInstance, Model, Scene, SceneRenderer, Vec3,
};

use crate::ui::logo::{WORD_BOT, WORD_TOP};

// ---------------------------------------------------------------------------
// The storyboard, as constants (ms from splash start)
// ---------------------------------------------------------------------------

/// Timeline length. Short enough that a returning user never waits on it,
/// long enough for the three-beat arc to read.
pub const TOTAL_MS: u32 = 1900;
/// Unconditional wall ceiling handed to the player (a stalled terminal
/// must never hold the app hostage).
pub const HARD_CUTOFF_MS: u32 = 2400;

/// Beat 1 → 2: the planes have travelled; alignment (and impact) here.
const ALIGN_START_MS: u32 = 900;
/// Beat 3: the wordmark lockup starts resolving.
const REVEAL_START_MS: u32 = 1180;
/// The composition holds, finished, before the handoff.
const HOLD_START_MS: u32 = 1760;
/// Plane `i` starts arriving at `i * STAGGER`.
const PLANE_STAGGER_MS: u32 = 120;
/// One plane's flight.
const PLANE_ARRIVAL_MS: u32 = 760;
/// The impact burst — the moment the three planes become one mark.
const BURST_AT_MS: u32 = ALIGN_START_MS;
/// Sparks kicked up when a plane lands.
const LAND_SPARKS: u32 = 6;

/// Position curve: back-out — the overshoot IS the "lock" (y1 > 1).
const EASE_SETTLE: [f32; 4] = [0.34, 1.56, 0.64, 1.0];
/// Fades (planes, letters, rules).
const EASE_FADE: [f32; 4] = [0.33, 1.0, 0.68, 1.0];
/// Tracking collapse on the wordmark: symmetric, calm.
const EASE_TRACKING: [f32; 4] = [0.83, 0.0, 0.17, 1.0];
/// Camera: starts angled so the planes read as planes, ends near-frontal
/// so the A silhouette locks (degrees; Depth lane).
const CAMERA_YAW_DEG: (f32, f32) = (-34.0, -5.0);
const CAMERA_PITCH_DEG: f32 = 8.0;
const CAMERA_DOLLY: (f32, f32) = (4.1, 3.15);
/// Afterglow decay per 100 ms: how much of the previous frame survives
/// into this one. Tight on purpose — a long tail smears into a comb of
/// ghost copies instead of reading as speed.
const AFTERGLOW_DECAY_PER_100MS: f32 = 0.42;

// ---------------------------------------------------------------------------
// The lockup (typography). Widths are display cells.
// ---------------------------------------------------------------------------

/// Under the wordmark: what this thing IS, in one line.
pub const TAGLINE: &str = "durable coding agents, live in your terminal";
/// The house line. Credits the engine because the engine is the pitch.
pub const FOOTER: &str = "abstractframework.ai · rendered by AbstractTUI";
/// Bottom-right affordance, from 300 ms (never promise a skip you have
/// not armed: the player checks input between every frame).
pub const SKIP_HINT: &str = "press any key to skip";
/// One-row wordmark for panes too narrow for the half-block lockup.
const WORDMARK_PLAIN: &str = "AbstractCode";
/// Letter tracking on the plain wordmark: airy → snug.
const PLAIN_TRACKING: (u16, u16) = (3, 1);

// ---------------------------------------------------------------------------
// The launch entry point
// ---------------------------------------------------------------------------

/// Play the boot animation on the real terminal, once, before the app
/// loop takes the screen. Returns `Err(reason)` when the environment (or
/// the operator's preference) says not to — the caller logs nothing and
/// goes straight to the app: a splash that cannot be skipped, disabled,
/// or degraded is a bug, not an identity.
///
/// `enabled` is the persisted `--animation` preference. Everything else
/// is the engine's own boot gate (a real tty, `NO_COLOR`, `TERM=dumb`,
/// `ABSTRACTTUI_NO_SPLASH`, and the capability report's dumb verdict).
///
/// Call it AFTER `App::mount` and before `App::run`: mounting has
/// already dispatched the boot fetches to the worker thread, so the
/// animation runs while the gateway probe, catalog and tool inventory
/// land — the splash costs wall-clock the client was spending anyway.
pub fn play_boot(enabled: bool) -> Result<SplashOutcome, &'static str> {
    if !enabled {
        return Err("animation disabled (--animation off)");
    }
    let caps = Capabilities::detect_env();
    abstracttui::boot::should_splash(&caps)?;
    let theme = abstracttui::app::current_theme();
    let mut term = new_terminal().map_err(|_| "no terminal")?;
    // A splash owns nothing but the screen: no mouse tracking, no paste
    // brackets, no focus reports — the app arms those for itself when it
    // enters, and arming them twice risks leaving them armed on exit.
    let opts = EnterOptions {
        alternate_screen: true,
        hide_cursor: true,
        mouse: MouseMode::Off,
        bracketed_paste: false,
        focus_events: false,
        kitty_keyboard: KittyFlags(0),
    };
    if term.enter(&opts).is_err() {
        return Err("could not enter raw mode");
    }
    let mut source = BootSplash::new(Lane::for_caps(&caps));
    let mut io = TerminalIo::new(&mut *term);
    let present = abstracttui::boot::player::splash_present_caps(&caps);
    let t0 = std::time::Instant::now();
    let mut clock = move || t0.elapsed().as_millis() as u64;
    let options = SplashOptions {
        fps: 30,
        total_ms: TOTAL_MS,
        hard_cutoff_ms: HARD_CUTOFF_MS,
        ..SplashOptions::default()
    };
    let outcome = play(&mut io, &mut source, theme, &present, &options, &mut clock);
    // The retained (non-deliberate) events are dropped WITH INTENT: the
    // app has not entered yet, so nothing it owns has been asked for —
    // it runs its own capability probe when the driver enters.
    let _ = io.finish();
    let _ = term.leave();
    outcome.map_err(|_| "terminal write failed")
}

#[cfg(unix)]
fn new_terminal() -> abstracttui::base::Result<Box<dyn abstracttui::term::Terminal>> {
    Ok(Box::new(abstracttui::term::UnixTerminal::new()?))
}

#[cfg(windows)]
fn new_terminal() -> abstracttui::base::Result<Box<dyn abstracttui::term::Terminal>> {
    Ok(Box::new(abstracttui::term::WindowsTerminal::new()?))
}

// ---------------------------------------------------------------------------
// Lane selection
// ---------------------------------------------------------------------------

/// Which renderer draws the mark. Both share the lockup and the beats.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lane {
    /// Software 3D: perspective planes, shading, depth fog, trails.
    Depth,
    /// Antialiased cell mosaic — no 3D, no truecolor requirement.
    Cells,
}

impl Lane {
    /// The lane a terminal earns. The 3D mark's gradient + depth fog
    /// need real color depth; 256-color grounds quantize it into mud,
    /// so they get the flat mark (which is designed for them).
    pub fn for_caps(caps: &abstracttui::term::Capabilities) -> Lane {
        if caps.truecolor {
            Lane::Depth
        } else {
            Lane::Cells
        }
    }
}

// ---------------------------------------------------------------------------
// The frame source
// ---------------------------------------------------------------------------

/// AbstractCode's boot animation as a splash frame source.
pub struct BootSplash {
    lane: Lane,
    surface: Surface,
    /// Depth lane: the software 3D pass (z-buffered, lambert-shaded).
    scene: SceneRenderer,
    fb: Framebuffer,
    /// Cells lane: the half-block raster target (w × 2h pixels).
    raster: Bitmap,
    /// Cells lane: afterglow buffer (previous frames, decayed).
    trail: Bitmap,
    /// Spark field (both lanes draw it at cell level).
    field: ParticleField,
    /// Fixed-step simulation clock — same t-sequence, same pixels.
    sim_t: f32,
    sim_size: Size,
    last_t: f32,
}

/// Particle sim quantum: the field advances in fixed steps up to the
/// requested `t`, so frame drops (and a test asking for one arbitrary
/// frame) produce identical output.
const SIM_STEP: f32 = 1.0 / 30.0;

impl BootSplash {
    pub fn new(lane: Lane) -> BootSplash {
        BootSplash {
            lane,
            surface: Surface::new(Size::new(0, 0), Cell::EMPTY),
            scene: SceneRenderer::new(),
            fb: Framebuffer::new(0, 0),
            raster: Bitmap::default(),
            trail: Bitmap::default(),
            field: new_field(),
            sim_t: 0.0,
            sim_size: Size::new(0, 0),
            last_t: 0.0,
        }
    }
}

/// Spark posture: light drag, a whisper of gravity — sparks arc and
/// settle rather than fly off. Seeded: deterministic replay.
fn new_field() -> ParticleField {
    let mut field = ParticleField::new(0xC0DE_5A17);
    field.gravity = (0.0, 2.6);
    field.drag = 0.82;
    field
}

fn ease(params: [f32; 4], k: f32) -> f32 {
    Easing::CubicBezier(params[0], params[1], params[2], params[3]).eval(k.clamp(0.0, 1.0))
}

/// Progress of `ms` through `[start, start+dur]`, clamped 0..=1.
fn window(ms: f32, start: f32, dur: f32) -> f32 {
    if dur <= 0.0 {
        return if ms >= start { 1.0 } else { 0.0 };
    }
    ((ms - start) / dur).clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

impl SplashFrameSource for BootSplash {
    fn render(&mut self, t: f32, size: Size, theme: &Theme) -> &Surface {
        if self.surface.size() != size {
            self.surface = Surface::new(size, Cell::EMPTY);
        }
        let ms = t * 1000.0;
        let plan = Layout::for_size(size);

        self.draw_ground(size, theme);
        // Under the mark, so the wave reads as emanating from BEHIND it
        // (and can never punch a hole in the letterform).
        self.draw_shockwave(ms, size, &plan, theme);
        match self.lane {
            Lane::Depth => self.draw_mark_depth(ms, &plan, theme),
            Lane::Cells => self.draw_mark_cells(ms, &plan, theme),
        }

        self.draw_sparks(t, size, &plan);
        self.draw_lockup(ms, size, &plan, theme);
        &self.surface
    }
}

// ---------------------------------------------------------------------------
// Layout: one plan, both lanes, honest degradation
// ---------------------------------------------------------------------------

/// Where every element sits for a given terminal size. Rows are absolute;
/// `None` means "this pane cannot carry that element" — it is DROPPED,
/// never clipped into glyph soup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// Mark box (Cells lane draws into it; Depth lane frames its own).
    pub mark: Rect,
    /// Wordmark rows: 2 rows of half-block letterforms, or 1 plain row.
    pub wordmark_y: i32,
    pub wordmark_rows: i32,
    pub wordmark_w: i32,
    pub rule_y: i32,
    pub tagline_y: Option<i32>,
    pub footer_y: Option<i32>,
    /// Bottom chrome row (version left, skip hint right).
    pub chrome_y: Option<i32>,
}

impl Layout {
    /// The lockup is BOTTOM-anchored (identical in both lanes); the mark
    /// takes the space that remains above it.
    pub fn for_size(size: Size) -> Layout {
        let (w, h) = (size.w, size.h);
        let word_w = text::width(WORD_TOP);
        let big = w >= word_w + 4 && h >= 16;
        let wordmark_rows = if big { 2 } else { 1 };
        let wordmark_w = if big {
            word_w
        } else {
            text::width(WORDMARK_PLAIN)
        };
        // Bottom chrome only where it cannot collide with the footer.
        let chrome_y = if h >= 12 { Some(h - 1) } else { None };
        let mut y = chrome_y.map(|c| c - 1).unwrap_or(h - 1);
        let footer_y = if h >= 18 && w >= text::width(FOOTER) + 2 {
            let row = y;
            y -= 1;
            Some(row)
        } else {
            None
        };
        let tagline_y = if h >= 14 && w >= text::width(TAGLINE) + 2 {
            let row = y;
            y -= 1;
            Some(row)
        } else {
            None
        };
        let rule_y = y;
        let wordmark_y = rule_y - wordmark_rows;
        // The mark fills what is left, with a one-row breath under it.
        let mark_h = (wordmark_y - 1).max(0);
        let mark = Rect::new(0, 0, w, mark_h);
        Layout {
            mark,
            wordmark_y,
            wordmark_rows,
            wordmark_w,
            rule_y,
            tagline_y,
            footer_y,
            chrome_y,
        }
    }
}

// ---------------------------------------------------------------------------
// Ground, mark, sparks (Cells lane)
// ---------------------------------------------------------------------------

impl BootSplash {
    /// Theme ground with a radial vignette toward the house field blue —
    /// the stage the mark stands on, on every theme.
    fn draw_ground(&mut self, size: Size, theme: &Theme) {
        let bg = theme.tokens.bg;
        let (cx, cy) = ((size.w as f32 - 1.0) * 0.5, (size.h as f32 - 1.0) * 0.5);
        let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
        for y in 0..size.h {
            for x in 0..size.w {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let k = (dx * dx + dy * dy).sqrt() / max_r * 0.14;
                let g = mix(bg, BRAND_FIELD, k);
                self.surface
                    .set(x, y, Cell::new(Glyph::SPACE).with_fg(g).with_bg(g));
            }
        }
    }

    /// The three planes, rasterized with coverage antialiasing into a
    /// half-block bitmap (1 cell = 1 × 2 pixels) and mosaicked to cells.
    /// Only lit cells are written, so sparks under the mark survive.
    fn draw_mark_cells(&mut self, ms: f32, plan: &Layout, theme: &Theme) {
        let box_ = plan.mark;
        if box_.w < 12 || box_.h < 3 {
            return; // no stage, no mark
        }
        // Half-block density (1 cell = 1 × 2 px), rendered at 2× and
        // box-downsampled: the strokes are diagonal, and a diagonal at
        // cell resolution is a staircase without the extra samples.
        let (pw, ph) = (box_.w as u32 * SS, (box_.h * 2) as u32 * SS);
        if self.raster.width() != pw || self.raster.height() != ph {
            self.raster = Bitmap::new(pw, ph, Rgba::TRANSPARENT);
        }
        self.raster.fill(Rgba::TRANSPARENT);

        // The mark's own box, centered in the stage. Proportions are the
        // letterform's, not the pane's: an A is taller than it is wide.
        let mh = (ph as f32 * 0.88).min(pw as f32 * 0.85 / A_ASPECT);
        let mw = mh * A_ASPECT;
        let ox = (pw as f32 - mw) * 0.5;
        let oy = (ph as f32 - mh) * 0.5;

        for (i, plane) in planes(mw, mh).iter().enumerate() {
            let start = (i as u32 * PLANE_STAGGER_MS) as f32;
            let k = window(ms, start, PLANE_ARRIVAL_MS as f32);
            if k <= 0.0 {
                continue;
            }
            let travel = ease(EASE_SETTLE, k);
            let alpha = ease(EASE_FADE, window(ms, start, PLANE_ARRIVAL_MS as f32 * 0.45));
            let dx = ox + plane.from.0 * (1.0 - travel);
            let dy = oy + plane.from.1 * (1.0 - travel);
            stroke(
                &mut self.raster,
                (plane.a.0 + dx, plane.a.1 + dy),
                (plane.b.0 + dx, plane.b.1 + dy),
                plane.thickness,
                alpha,
                ox,
                mw,
            );
        }

        // Afterglow: the previous frames, decayed, under the live one.
        let decay = self.trail_decay(ms);
        let live = downsample(&self.raster);
        merge_trail(&mut self.trail, &live, decay);
        blit_mosaic(&mut self.surface, &self.trail, box_, theme.tokens.bg);
    }

    /// The three planes as SLABS in a real scene: perspective, lambert
    /// shading, z-buffer, depth fog. Same beats, same silhouette, same
    /// gradient — this lane just has volume and a camera move.
    fn draw_mark_depth(&mut self, ms: f32, plan: &Layout, theme: &Theme) {
        let box_ = plan.mark;
        if box_.w < 12 || box_.h < 3 {
            return;
        }
        let (pw, ph) = (box_.w as u32 * SS, (box_.h * 2) as u32 * SS);
        if self.fb.width() != pw || self.fb.height() != ph {
            self.fb = Framebuffer::new(pw, ph);
        }
        let model = build_slabs(ms);
        // Camera: angled enough at the start to read the planes as
        // LAYERS, near-frontal at the end so the silhouette locks.
        let k = ease(EASE_TRACKING, window(ms, 0.0, REVEAL_START_MS as f32));
        let camera = Camera::orbit(
            Vec3::new(0.0, 0.0, 0.0),
            lerp(CAMERA_DOLLY.0, CAMERA_DOLLY.1, k),
            lerp(CAMERA_YAW_DEG.0, CAMERA_YAW_DEG.1, k).to_radians(),
            CAMERA_PITCH_DEG.to_radians(),
        );
        let mut scene = Scene::new(&model, camera);
        // Emissive posture: high ambient so the ramp reads as the brand
        // gradient, a gentle key so the slabs still separate in depth.
        scene.light = Light {
            direction: Vec3::new(-0.3, -0.6, -0.75),
            ambient: 0.80,
            diffuse: 0.34,
        };
        scene.background = Rgba::TRANSPARENT;
        scene.double_sided = true;
        self.scene.render(&scene, &mut self.fb);
        self.fb.depth_fog(theme.tokens.bg, 0.22);
        let decay = self.trail_decay(ms);
        let live = downsample(self.fb.bitmap());
        merge_trail(&mut self.trail, &live, decay);
        blit_mosaic(&mut self.surface, &self.trail, box_, theme.tokens.bg);
    }

    /// How much of the trail survives this frame, given the wall-clock
    /// step since the last one. A rewind (a test probing an arbitrary
    /// frame) replays from cold rather than inheriting a stale tail.
    fn trail_decay(&mut self, ms: f32) -> f32 {
        let t = ms / 1000.0;
        if t < self.last_t {
            self.trail.fill(Rgba::TRANSPARENT);
        }
        let dt = (t - self.last_t).clamp(0.0, 0.5);
        self.last_t = t;
        AFTERGLOW_DECAY_PER_100MS.powf((dt * 10.0).max(0.0))
    }

    /// The impact shockwave: at the moment the three planes lock, a
    /// hairline fires out of the mark's waist to the full width and
    /// fades — the same rule that will settle under the wordmark a beat
    /// later, so the two read as one motif rather than two decorations.
    fn draw_shockwave(&mut self, ms: f32, size: Size, plan: &Layout, theme: &Theme) {
        if plan.mark.h < 4 {
            return;
        }
        let k = window(ms, BURST_AT_MS as f32, 420.0);
        if k <= 0.0 || k >= 1.0 {
            return;
        }
        let reach = ease(EASE_FADE, k) * size.w as f32 * 0.5;
        let alpha = (1.0 - k).powi(2);
        let y = plan.mark.y + (plan.mark.h as f32 * 0.62) as i32;
        let cx = size.w / 2;
        let bg = theme.tokens.bg;
        for x in (cx - reach as i32).max(0)..=(cx + reach as i32).min(size.w - 1) {
            let ramp = brand_ramp(x as f32 / (size.w - 1).max(1) as f32);
            let ink = mix(bg, ramp, alpha * 0.55);
            self.surface.draw_text(x, y, "─", Style::new().fg(ink));
        }
    }

    /// Landing kicks + the alignment burst, at cell scale, drawn over the
    /// ground and under the lockup.
    fn draw_sparks(&mut self, t: f32, size: Size, plan: &Layout) {
        if size.w < 20 || plan.mark.h < 3 {
            return;
        }
        if t < self.sim_t || size != self.sim_size {
            self.field = new_field();
            self.sim_t = 0.0;
            self.sim_size = size;
        }
        let cx = size.w as f32 * 0.5;
        let cy = plan.mark.y as f32 + plan.mark.h as f32 * 0.58;
        let spread = (plan.mark.h as f32 * 1.1).max(4.0);
        while self.sim_t < t {
            let next = (self.sim_t + SIM_STEP).min(t);
            for i in 0..3u32 {
                let land = (i * PLANE_STAGGER_MS + PLANE_ARRIVAL_MS) as f32 / 1000.0;
                if self.sim_t < land && land <= next {
                    let x = match i {
                        0 => cx - spread * 0.5,
                        1 => cx + spread * 0.5,
                        _ => cx,
                    };
                    self.field.spawn(Burst {
                        origin: (x, cy),
                        count: LAND_SPARKS as usize,
                        speed: (3.0, 8.0),
                        life: (0.25, 0.5),
                        colors: [brand_ramp(0.0), brand_ramp(0.5), brand_ramp(1.0)],
                    });
                }
            }
            let burst_at = BURST_AT_MS as f32 / 1000.0;
            if self.sim_t < burst_at && burst_at <= next {
                self.field.spawn(Burst {
                    origin: (cx, cy),
                    count: 14,
                    speed: (6.0, 15.0),
                    life: (0.3, 0.5),
                    colors: [brand_ramp(0.0), brand_ramp(0.5), brand_ramp(1.0)],
                });
            }
            self.field.step(next - self.sim_t);
            self.sim_t = next;
        }
        self.field.render(&mut self.surface);
    }
}

/// One arriving plane: a thick segment in mark-box pixel space plus the
/// off-stage offset it flies in from.
struct Plane {
    a: (f32, f32),
    b: (f32, f32),
    thickness: f32,
    from: (f32, f32),
}

/// The three planes of the house mark, in a box of `mw × mh` pixels:
/// two ascending legs and the crossbar that turns them into an "A".
fn planes(mw: f32, mh: f32) -> [Plane; 3] {
    let leg = (mh * 0.115).max(1.2);
    [
        // Left leg — arrives from off-stage left, below.
        Plane {
            a: (mw * 0.5, mh * 0.03),
            b: (mw * 0.06, mh * 0.97),
            thickness: leg,
            from: (-mw * 1.25, mh * 0.55),
        },
        // Right leg — from off-stage right, below.
        Plane {
            a: (mw * 0.5, mh * 0.03),
            b: (mw * 0.94, mh * 0.97),
            thickness: leg,
            from: (mw * 1.25, mh * 0.55),
        },
        // Crossbar — rises last, straight up into the lock.
        Plane {
            a: (mw * 0.27, mh * 0.68),
            b: (mw * 0.73, mh * 0.68),
            thickness: leg * 0.85,
            from: (0.0, mh * 1.35),
        },
    ]
}

/// Rasterize a thick segment with coverage antialiasing, colored by the
/// house ramp across the mark's width (so the whole mark reads as ONE
/// gradient object rather than three tinted bars).
fn stroke(
    dst: &mut Bitmap,
    a: (f32, f32),
    b: (f32, f32),
    thickness: f32,
    alpha: f32,
    ramp_x0: f32,
    ramp_w: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    let half = thickness * 0.5;
    let (w, h) = (dst.width() as i32, dst.height() as i32);
    let pad = half.ceil() as i32 + 1;
    let x0 = (a.0.min(b.0) as i32 - pad).max(0);
    let x1 = (a.0.max(b.0) as i32 + pad).min(w - 1);
    let y0 = (a.1.min(b.1) as i32 - pad).max(0);
    let y1 = (a.1.max(b.1) as i32 + pad).min(h - 1);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance to the segment (the classic clamped projection).
            let k = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
            let (qx, qy) = (a.0 + dx * k, a.1 + dy * k);
            let d = ((px - qx).powi(2) + (py - qy).powi(2)).sqrt();
            let cover = (half + 0.5 - d).clamp(0.0, 1.0) * alpha;
            if cover <= 0.004 {
                continue;
            }
            let ramp_t = if ramp_w > 0.0 {
                ((px - ramp_x0) / ramp_w).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let c = brand_ramp(ramp_t);
            let lit = Rgba::new(c.r, c.g, c.b, (cover * 255.0) as u8);
            let prev = dst.get(x as u32, y as u32).unwrap_or(Rgba::TRANSPARENT);
            if lit.a >= prev.a {
                dst.set(x as u32, y as u32, lit);
            }
        }
    }
}

/// Supersampling factor for both marks: render at 2×, box-downsample to
/// half-block pixels. The whole mark is ~40 × 40 px at cell density —
/// small enough that 4 samples per pixel cost nothing and buy every
/// diagonal edge in the composition.
const SS: u32 = 2;

/// Width-to-height of the letterform. The house A is upright: matching
/// the pane's aspect instead would splay it into a triangle.
const A_ASPECT: f32 = 0.72;

/// One box-filter halving per supersample step.
fn downsample(src: &Bitmap) -> Bitmap {
    let mut out = src.box_halved();
    let mut left = SS / 2;
    while left > 1 {
        out = out.box_halved();
        left /= 2;
    }
    out
}

/// Decay what the trail holds and keep whichever of (faded trail, live
/// frame) is brighter — a motion afterglow for the price of one pass.
fn merge_trail(trail: &mut Bitmap, live: &Bitmap, decay: f32) {
    if trail.width() != live.width() || trail.height() != live.height() {
        *trail = Bitmap::new(live.width(), live.height(), Rgba::TRANSPARENT);
    }
    let live_px: Vec<Rgba> = live.pixels().to_vec();
    for (i, px) in trail.pixels_mut().iter_mut().enumerate() {
        let faded = Rgba::new(px.r, px.g, px.b, (px.a as f32 * decay) as u8);
        let now = live_px[i];
        *px = if now.a >= faded.a { now } else { faded };
    }
}

/// Mosaic a half-block bitmap into `box_` — writing ONLY lit cells, so
/// the vignette (and the sparks drawn over it) survive underneath.
fn blit_mosaic(surface: &mut Surface, src: &Bitmap, box_: Rect, bg: Rgba) {
    let grid = mosaic::render(src, box_.w as u32, box_.h as u32, MosaicMode::HalfBlock);
    for row in 0..box_.h {
        for col in 0..box_.w {
            let cell = grid
                .get(col as u32, row as u32)
                .copied()
                .unwrap_or(MosaicCell::EMPTY);
            if cell.fg.is_transparent() && cell.bg.is_transparent() {
                continue;
            }
            let mut buf = [0u8; 4];
            let s: &str = cell.ch.encode_utf8(&mut buf);
            surface.draw_text(
                box_.x + col,
                box_.y + row,
                s,
                Style::new().fg(cell.fg.over(bg)).bg(cell.bg.over(bg)),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The mark in three dimensions (Depth lane)
// ---------------------------------------------------------------------------

/// Scene-unit geometry of the A: total height 2.0, feet at ±0.62, the
/// crossbar where the legs are 0.83 apart.
const A_HALF_H: f32 = 1.0;
const A_FOOT_X: f32 = 0.62;
const A_BAR_Y: f32 = -0.34;
const SLAB_T: f32 = 0.23;
const SLAB_D: f32 = 0.15;
/// Depth separation between the three slabs: what the opening yaw
/// reveals — three LAYERS, not one extruded letter.
const SLAB_DZ: f32 = 0.17;

/// One slab of the mark: a box of `size` cells standing at `at`, rotated
/// `rot` radians about Z, flying in from `from`, painted across `ramp` of
/// the house gradient.
struct Slab {
    size: (f32, f32),
    at: (f32, f32),
    rot: f32,
    from: Vec3,
    ramp: (f32, f32),
}

/// The three slabs at their `ms` poses: two legs and the crossbar, each
/// flying in from off-stage on the shared stagger.
fn build_slabs(ms: f32) -> Model {
    // Leg: a bar from foot (∓0.62, -1) to apex (0, +1).
    let leg_len = (A_FOOT_X * A_FOOT_X + (2.0 * A_HALF_H).powi(2)).sqrt();
    let lean = (A_FOOT_X / (2.0 * A_HALF_H)).atan();
    let bar_w = A_FOOT_X * (1.0 - (A_BAR_Y + A_HALF_H) / (2.0 * A_HALF_H)) * 2.0 + SLAB_T;
    let parts = [
        Slab {
            size: (SLAB_T, leg_len),
            at: (-A_FOOT_X * 0.5, 0.0),
            rot: -lean,
            from: Vec3::new(-5.8, -2.5, 0.5),
            ramp: (0.0, 0.46),
        },
        Slab {
            size: (SLAB_T, leg_len),
            at: (A_FOOT_X * 0.5, 0.0),
            rot: lean,
            from: Vec3::new(5.8, -2.5, -0.5),
            ramp: (1.0, 0.54),
        },
        Slab {
            size: (bar_w, SLAB_T * 0.86),
            at: (0.0, A_BAR_Y),
            rot: 0.0,
            from: Vec3::new(0.0, -4.8, 1.1),
            ramp: (0.30, 0.70),
        },
    ];
    let mut instances = Vec::with_capacity(3);
    for (i, part) in parts.into_iter().enumerate() {
        let (w, h) = part.size;
        let (x, y) = part.at;
        let (rot, from, ramp) = (part.rot, part.from, part.ramp);
        let start = (i as u32 * PLANE_STAGGER_MS) as f32;
        let k = window(ms, start, PLANE_ARRIVAL_MS as f32);
        if k <= 0.0 {
            continue; // not yet in flight: absent from the scene entirely
        }
        let travel = ease(EASE_SETTLE, k);
        let z = (i as f32 - 1.0) * SLAB_DZ;
        let pos = Vec3::new(x, y, z) + from * (1.0 - travel);
        let mut mesh = cuboid(w, h, SLAB_D);
        paint_ramp(&mut mesh, h.max(w), rot.abs() > 0.001, ramp);
        mesh.material = Some(0);
        instances.push(MeshInstance {
            data: mesh,
            world: Mat4::translate(pos).mul(&Mat4::rotate_z(rot)),
            source_node: None,
        });
    }
    Model {
        instances,
        materials: vec![MaterialData::default()],
        rig: None,
        warnings: Vec::new(),
    }
}

/// Vertex colors along the slab's long axis, spanning `ramp` of the
/// house gradient — so the assembled mark reads as ONE gradient object
/// running red (left foot) → violet (apex) → blue (right foot).
/// Brand stops are sRGB; vertex colors are linear.
fn paint_ramp(mesh: &mut abstracttui::three::MeshData, span: f32, along_y: bool, ramp: (f32, f32)) {
    let colors = mesh
        .positions
        .iter()
        .map(|p| {
            let axis = if along_y { p[1] } else { p[0] };
            let k = (axis / span + 0.5).clamp(0.0, 1.0);
            let c = brand_ramp(lerp(ramp.0, ramp.1, k));
            [
                srgb8_to_linear(c.r),
                srgb8_to_linear(c.g),
                srgb8_to_linear(c.b),
                1.0,
            ]
        })
        .collect();
    mesh.colors = Some(colors);
}

// ---------------------------------------------------------------------------
// The lockup: wordmark, rule, tagline, footer, chrome
// ---------------------------------------------------------------------------

impl BootSplash {
    fn draw_lockup(&mut self, ms: f32, size: Size, plan: &Layout, theme: &Theme) {
        let tk = &theme.tokens;
        let bg = tk.bg;
        let reveal = REVEAL_START_MS as f32;
        let span = (HOLD_START_MS - REVEAL_START_MS) as f32;

        // Skip hint + version: the chrome row, from 300 ms. It is the
        // only thing on screen during the first beat, and it is faint.
        if let Some(y) = plan.chrome_y {
            let a = ease(EASE_FADE, window(ms, 300.0, 250.0));
            if a > 0.0 {
                let ink = mix(bg, tk.text_faint, a);
                let version = format!("abstractcode-tui {}", crate::cli::VERSION);
                if size.w >= text::width(&version) + text::width(SKIP_HINT) + 4 {
                    self.surface.draw_text(1, y, &version, Style::new().fg(ink));
                }
                if size.w >= text::width(SKIP_HINT) + 2 {
                    let x = size.w - text::width(SKIP_HINT) - 1;
                    self.surface
                        .draw_text(x, y, SKIP_HINT, Style::new().fg(ink));
                }
            }
        }

        if ms < reveal {
            return;
        }
        let k = ((ms - reveal) / span.max(1.0)).clamp(0.0, 1.0);

        // Wordmark. Two-row half-block letterforms where they fit (the
        // SAME art the idle screen carries — the splash lands into it),
        // one plain row with a tracking collapse where they do not.
        if plan.wordmark_rows == 2 {
            let x0 = (size.w - plan.wordmark_w) / 2;
            for (row, art) in [WORD_TOP, WORD_BOT].into_iter().enumerate() {
                let y = plan.wordmark_y + row as i32;
                for (col, ch) in art.chars().enumerate() {
                    if ch == ' ' {
                        continue;
                    }
                    // Letters resolve left → right, 14 ms apart.
                    let a = ease(EASE_FADE, window(ms, reveal + col as f32 * 14.0, 220.0));
                    if a <= 0.0 {
                        continue;
                    }
                    let ink = mix(bg, tk.text, a);
                    self.surface.draw_text(
                        x0 + col as i32,
                        y,
                        &ch.to_string(),
                        Style::new().fg(ink),
                    );
                }
            }
        } else {
            let letters: Vec<char> = WORDMARK_PLAIN.chars().collect();
            let track = ease(EASE_TRACKING, k);
            let spacing = lerp(PLAIN_TRACKING.0 as f32, PLAIN_TRACKING.1 as f32, track);
            let step = spacing.max(1.0);
            let width_now = ((letters.len() as f32 - 1.0) * step) as i32 + 1;
            let x0 = (size.w - width_now) / 2;
            for (i, ch) in letters.iter().enumerate() {
                let a = ease(EASE_FADE, window(ms, reveal + i as f32 * 25.0, 200.0));
                if a <= 0.0 {
                    continue;
                }
                let ink = mix(bg, if i == 0 { tk.accent } else { tk.text }, a);
                self.surface.draw_text(
                    x0 + (i as f32 * step).round() as i32,
                    plan.wordmark_y,
                    &ch.to_string(),
                    Style::new().fg(ink),
                );
            }
        }

        // The rule: ignites under the wordmark and sweeps to its width,
        // carrying the house ramp end to end.
        let sweep = ease(EASE_FADE, window(ms, reveal + 60.0, 420.0));
        let full = plan.wordmark_w;
        let lit = (full as f32 * sweep).round() as i32;
        let rx = (size.w - full) / 2;
        for x in 0..lit {
            let c = brand_ramp(x as f32 / (full.max(1) - 1).max(1) as f32);
            self.surface
                .draw_text(rx + x, plan.rule_y, "─", Style::new().fg(c));
        }

        // Tagline + footer, trailing the wordmark.
        if let Some(y) = plan.tagline_y {
            let a = ease(EASE_FADE, window(ms, reveal + 180.0, 320.0));
            if a > 0.0 {
                let ink = mix(bg, tk.text_muted, a);
                let x = (size.w - text::width(TAGLINE)) / 2;
                self.surface.draw_text(x, y, TAGLINE, Style::new().fg(ink));
            }
        }
        if let Some(y) = plan.footer_y {
            let a = ease(EASE_FADE, window(ms, reveal + 300.0, 320.0));
            if a > 0.0 {
                let ink = mix(bg, tk.text_faint, a);
                let x = (size.w - text::width(FOOTER)) / 2;
                self.surface.draw_text(x, y, FOOTER, Style::new().fg(ink));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abstracttui::theme::default_theme;

    const LANES: [Lane; 2] = [Lane::Depth, Lane::Cells];
    const SIZE: Size = Size { w: 100, h: 30 };

    fn row_text(s: &Surface, y: i32) -> String {
        (0..s.width())
            .map(|x| {
                s.get(x, y)
                    .map(|c| s.glyph_str(c))
                    .filter(|t| !t.is_empty())
                    .and_then(|t| t.chars().next())
                    .unwrap_or(' ')
            })
            .collect()
    }

    fn screen(s: &Surface) -> String {
        (0..s.height())
            .map(|y| row_text(s, y))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Play the timeline at the design cadence up to `ms` — the beats
    /// must hold under the SAME stepping the player uses, not only for a
    /// cold single frame (the trail and the spark field carry history).
    fn play_to(lane: Lane, ms: u32, size: Size) -> BootSplash {
        let mut src = BootSplash::new(lane);
        let mut t = 0u32;
        while t < ms {
            src.render(t as f32 / 1000.0, size, default_theme());
            t += 33;
        }
        src.render(ms as f32 / 1000.0, size, default_theme());
        src
    }

    /// The opening frame is a QUIET GROUND on both lanes: the vignette is
    /// painted, but nothing has arrived yet — no mark, no chrome, no
    /// wordmark. (The first beat of the storyboard is stillness; a splash
    /// that starts mid-composition has no arrival to sell.)
    #[test]
    fn t0_is_a_quiet_ground() {
        for lane in LANES {
            let mut src = BootSplash::new(lane);
            let frame = src.render(0.0, SIZE, default_theme());
            assert!(
                screen(frame).trim().is_empty(),
                "[{lane:?}] t=0 must be bare ground, got:\n{}",
                screen(frame)
            );
            // ...but the ground IS painted: the vignette darkens outward.
            let center = frame.get(SIZE.w / 2, SIZE.h / 2).unwrap().bg;
            let corner = frame.get(0, 0).unwrap().bg;
            assert_ne!(center, corner, "[{lane:?}] the vignette is painted");
        }
    }

    /// The skip affordance waits 300 ms (a hint that flashes on frame one
    /// reads as an error), then stays for the rest of the timeline.
    #[test]
    fn skip_hint_appears_after_the_grace_and_stays() {
        for lane in LANES {
            let early = play_to(lane, 200, SIZE);
            let last = SIZE.h - 1;
            assert!(
                !row_text(&early.surface, last).contains(SKIP_HINT),
                "[{lane:?}] no hint inside the 300 ms grace"
            );
            for ms in [700, 1400, TOTAL_MS] {
                let src = play_to(lane, ms, SIZE);
                assert!(
                    row_text(&src.surface, last).contains(SKIP_HINT),
                    "[{lane:?}] hint missing at {ms} ms"
                );
            }
        }
    }

    /// The wordmark belongs to the LAST beat: nothing of the lockup may
    /// appear before the reveal, and all of it must have landed by the
    /// end of the timeline (a splash whose payoff needs the hold to
    /// finish is a splash that gets skipped before the payoff).
    #[test]
    fn wordmark_waits_for_the_reveal_and_lands_by_the_end() {
        for lane in LANES {
            let before = play_to(lane, REVEAL_START_MS - 60, SIZE);
            assert!(
                !screen(&before.surface).contains(TAGLINE),
                "[{lane:?}] tagline must wait for the reveal"
            );
            assert_eq!(
                row_text(&before.surface, Layout::for_size(SIZE).wordmark_y).trim(),
                "",
                "[{lane:?}] wordmark row is empty before the reveal"
            );
            let done = play_to(lane, TOTAL_MS, SIZE);
            let plan = Layout::for_size(SIZE);
            let top = row_text(&done.surface, plan.wordmark_y);
            assert_eq!(
                top.trim(),
                WORD_TOP,
                "[{lane:?}] the wordmark's first row is the idle screen's own art"
            );
            assert_eq!(
                row_text(&done.surface, plan.wordmark_y + 1).trim(),
                WORD_BOT
            );
            assert!(
                screen(&done.surface).contains(TAGLINE),
                "[{lane:?}] tagline"
            );
            assert!(screen(&done.surface).contains(FOOTER), "[{lane:?}] footer");
        }
    }

    /// The mark is there before the wordmark is: by the alignment beat
    /// both lanes have painted a substantial lit area in the mark box.
    /// (Pinned as a FLOOR, not an exact pixel count — the two renderers
    /// legitimately differ; what must never happen is an empty stage.)
    #[test]
    fn the_mark_is_drawn_by_the_alignment_beat() {
        for lane in LANES {
            let src = play_to(lane, ALIGN_START_MS, SIZE);
            let plan = Layout::for_size(SIZE);
            let lit = (plan.mark.y..plan.mark.y + plan.mark.h)
                .flat_map(|y| (0..SIZE.w).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let cell = src.surface.get(*x, *y).unwrap();
                    !src.surface.glyph_str(cell).trim().is_empty()
                })
                .count();
            assert!(
                lit > 40,
                "[{lane:?}] the mark must be on stage at the alignment beat (lit cells: {lit})"
            );
        }
    }

    /// Every frame is exactly the requested size, and hostile sizes
    /// (zero, one cell, one column) render instead of panicking — the
    /// splash runs before any layout guard the app has.
    #[test]
    fn hostile_sizes_render_instead_of_panicking() {
        for lane in LANES {
            for size in [
                Size::new(0, 0),
                Size::new(1, 1),
                Size::new(1, 40),
                Size::new(200, 1),
                Size::new(13, 7),
                Size::new(52, 16),
            ] {
                let mut src = BootSplash::new(lane);
                for ms in [0, 500, 1000, 1500, TOTAL_MS] {
                    let frame = src.render(ms as f32 / 1000.0, size, default_theme());
                    assert_eq!(frame.size(), size, "[{lane:?}] {size:?}");
                }
            }
        }
    }

    /// Honest degradation: the lockup DROPS rows it cannot fit (it never
    /// clips them into each other), every row it keeps is on screen, and
    /// no two elements are assigned the same row.
    #[test]
    fn layout_degrades_by_dropping_never_by_overlapping() {
        for h in 4..48 {
            for w in [30, 46, 52, 80, 100, 200] {
                let size = Size::new(w, h);
                let plan = Layout::for_size(size);
                let mut rows: Vec<i32> = vec![plan.rule_y];
                rows.extend((0..plan.wordmark_rows).map(|i| plan.wordmark_y + i));
                rows.extend(plan.tagline_y);
                rows.extend(plan.footer_y);
                rows.extend(plan.chrome_y);
                for y in &rows {
                    assert!(
                        (0..h).contains(y),
                        "{size:?}: row {y} is off screen ({plan:?})"
                    );
                }
                let mut sorted = rows.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(
                    sorted.len(),
                    rows.len(),
                    "{size:?}: two elements share a row"
                );
                assert!(
                    plan.mark.h == 0 || plan.mark.y + plan.mark.h <= plan.wordmark_y,
                    "{size:?}: the mark box must clear the wordmark"
                );
                // Wide panes get the half-block lockup; narrow ones the
                // one-row wordmark — never a clipped half of the art.
                let expect_big = w >= text::width(WORD_TOP) + 4 && h >= 16;
                assert_eq!(
                    plan.wordmark_rows,
                    if expect_big { 2 } else { 1 },
                    "{size:?}"
                );
            }
        }
    }

    /// Same t-sequence, same pixels: the trail and the particle field are
    /// history-bearing, so "deterministic" has to mean "deterministic
    /// under identical stepping" — which is what the player guarantees.
    #[test]
    fn identical_playback_is_identical_output() {
        for lane in LANES {
            let a = play_to(lane, 1500, SIZE);
            let b = play_to(lane, 1500, SIZE);
            assert_eq!(screen(&a.surface), screen(&b.surface), "[{lane:?}]");
        }
    }

    /// The preference is a veto, checked before ANY terminal work: with
    /// the animation off, `play_boot` refuses without touching the tty.
    #[test]
    fn the_animation_preference_is_a_veto() {
        assert!(
            play_boot(false).unwrap_err().contains("animation disabled"),
            "--animation off must refuse before opening a terminal"
        );
    }

    /// The storyboard's beats stay ordered. A constant edited into the
    /// wrong order would produce a splash that reveals its wordmark
    /// before the mark arrives — this is the pin that says so.
    #[test]
    fn the_timeline_is_ordered() {
        const {
            assert!(PLANE_STAGGER_MS * 2 + PLANE_ARRIVAL_MS <= ALIGN_START_MS + 240);
            assert!(ALIGN_START_MS < REVEAL_START_MS);
            assert!(REVEAL_START_MS < HOLD_START_MS);
            assert!(HOLD_START_MS < TOTAL_MS);
            assert!(TOTAL_MS < HARD_CUTOFF_MS, "the wall must sit past the end");
        }
    }
}
