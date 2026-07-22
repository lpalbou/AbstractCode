//! App-scale reactive state: a store struct of signals provided as context.
//!
//! All signals are written on the UI thread only — worker threads post
//! closures through `WakeHandle` (the engine rule).

use std::sync::Arc;
use std::time::Instant;

use abstracttui::prelude::*;
use abstracttui::widgets::Bitmap;

use crate::transcript::Fold;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Starting,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    Unknown,
    Ok,
    Down(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Workflow {
    pub bundle_id: String,
    pub flow_id: String,
    pub name: String,
    pub description: String,
}

impl Workflow {
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("{}:{}", self.bundle_id, self.flow_id)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderInfo {
    pub name: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// Gateway grouping ("files", "web", "system", MCP server name, …).
    pub toolset: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub trust: String,
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    pub description: String,
    pub auth_required: bool,
}

/// Prompt-cache posture for the effective provider/model route.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheInfo {
    pub provider: String,
    pub model: String,
    pub supported: bool,
    /// "keyed" | "local" | … (the gateway's capability answer).
    pub mode: String,
}

#[derive(Clone)]
pub struct ImageEntry {
    pub artifact_id: String,
    pub bitmap: Option<Arc<Bitmap>>,
    pub error: String,
}

/// Session-scope token totals (across runs; per-run stats live in the fold).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub runs: u64,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub fold: Signal<Fold>,
    pub phase: Signal<Phase>,
    pub conn: Signal<Conn>,
    pub session_id: Signal<String>,
    pub run_id: Signal<String>,
    pub workflow: Signal<Workflow>,
    pub workflows: Signal<Vec<Workflow>>,
    pub provider: Signal<String>,
    pub model: Signal<String>,
    pub providers: Signal<Vec<ProviderInfo>>,
    pub tools: Signal<Vec<ToolInfo>>,
    pub tools_error: Signal<String>,
    /// Tools the user switched OFF (persisted; `/tools`).
    pub disabled_tools: Signal<Vec<String>>,
    /// Gateway skill shelf (`/skills`).
    pub skills_catalog: Signal<Vec<SkillInfo>>,
    pub skills_error: Signal<String>,
    /// Skill names attached to every run (persisted; `input_data.skills`).
    pub selected_skills: Signal<Vec<String>>,
    /// Gateway MCP server registry (`/mcp`), plus its honest empty-state note.
    pub mcp_servers: Signal<Vec<McpServer>>,
    pub mcp_note: Signal<String>,
    /// Prompt-cache capability for the effective route (None until probed).
    pub cache: Signal<Option<CacheInfo>>,
    /// The gateway's configured default text route (provider, model) — what
    /// "gateway defaults" actually resolves to (capability input.text route).
    pub default_route: Signal<(String, String)>,
    pub images: Signal<Vec<ImageEntry>>,
    pub totals: Signal<SessionTotals>,
    pub run_started: Signal<Option<Instant>>,
    pub elapsed_secs: Signal<u64>,
    /// Pending toast texts; a UI effect drains them into Toast overlays.
    pub notices: Signal<Vec<String>>,
    /// Bumped by Esc; two within a second cancels the run.
    pub last_esc: Signal<Option<Instant>>,
    /// Show reasoning detail (thinking blocks, tool result previews).
    /// Hidden = the clean answers-only view; toggled by Ctrl+D //details.
    pub show_details: Signal<bool>,
    /// Auto-approve tool batches ("approve all"). SESSION-SCOPED and
    /// in-memory only — deliberately never persisted (a durable blanket
    /// approval is a footgun); reset by /new and session switches.
    pub auto_approve: Signal<bool>,
    /// The active run tree is PAUSED on the gateway (durable /pause).
    pub paused: Signal<bool>,
}

impl Store {
    pub fn create(cx: Scope) -> Store {
        Store {
            fold: cx.signal(Fold::new()),
            phase: cx.signal(Phase::Idle),
            conn: cx.signal(Conn::Unknown),
            session_id: cx.signal(String::new()),
            run_id: cx.signal(String::new()),
            workflow: cx.signal(Workflow::default()),
            workflows: cx.signal(Vec::new()),
            provider: cx.signal(String::new()),
            model: cx.signal(String::new()),
            providers: cx.signal(Vec::new()),
            tools: cx.signal(Vec::new()),
            tools_error: cx.signal(String::new()),
            disabled_tools: cx.signal(Vec::new()),
            skills_catalog: cx.signal(Vec::new()),
            skills_error: cx.signal(String::new()),
            selected_skills: cx.signal(Vec::new()),
            mcp_servers: cx.signal(Vec::new()),
            mcp_note: cx.signal(String::new()),
            cache: cx.signal(None),
            default_route: cx.signal((String::new(), String::new())),
            images: cx.signal(Vec::new()),
            totals: cx.signal(SessionTotals::default()),
            run_started: cx.signal(None),
            elapsed_secs: cx.signal(0),
            notices: cx.signal(Vec::new()),
            last_esc: cx.signal(None),
            show_details: cx.signal(true),
            auto_approve: cx.signal(false),
            paused: cx.signal(false),
        }
    }

    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        self.notices.update(|n| n.push(text));
    }

    pub fn image_for(&self, artifact_id: &str) -> Option<ImageEntry> {
        self.images
            .with(|imgs| imgs.iter().find(|e| e.artifact_id == artifact_id).cloned())
    }

    /// Insert or replace the image entry for its artifact id — UPSERT,
    /// never append: session revisits re-request the same artifacts (the
    /// fold's dedup resets with the fold), and append-only entries both
    /// leaked bitmaps and let a transient error entry permanently shadow
    /// a later successful fetch, because `image_for` returns the first
    /// match (adversary finding 7, 2026-07-22).
    ///
    /// Success is STICKY: artifacts are immutable, so an already-decoded
    /// bitmap stays valid forever — a transient re-fetch error must not
    /// clobber it (last-wins would degrade a rendered image to an error
    /// card on a gateway hiccup). Successful decodes always replace.
    pub fn upsert_image(&self, entry: ImageEntry) {
        self.images.update(|imgs| {
            match imgs.iter_mut().find(|e| e.artifact_id == entry.artifact_id) {
                Some(slot) => {
                    if entry.bitmap.is_some() || slot.bitmap.is_none() {
                        *slot = entry;
                    }
                }
                None => imgs.push(entry),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abstracttui::widgets::Bitmap;

    #[test]
    fn image_upsert_replaces_by_artifact_id_and_keeps_good_bitmaps() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            let entry = |id: &str, bitmap: Option<Arc<Bitmap>>, error: &str| ImageEntry {
                artifact_id: id.into(),
                bitmap,
                error: error.into(),
            };
            let bitmap = || {
                Some(Arc::new(Bitmap::new(
                    1,
                    1,
                    abstracttui::prelude::Rgba::BLACK,
                )))
            };

            // A transient error entry must NOT permanently shadow a later
            // successful fetch (`image_for` returns the first match).
            store.upsert_image(entry("a1", None, "image fetch failed: timeout"));
            store.upsert_image(entry("a2", None, ""));
            store.upsert_image(entry("a1", bitmap(), ""));
            assert_eq!(
                store.images.with_untracked(|v| v.len()),
                2,
                "upsert never grows"
            );
            let a1 = store.image_for("a1").expect("entry exists");
            assert!(a1.bitmap.is_some(), "success replaced the error entry");
            assert!(a1.error.is_empty());

            // Success is sticky: a transient error on a session-revisit
            // re-fetch must not clobber the already-decoded bitmap
            // (artifacts are immutable; the old pixels are still true).
            store.upsert_image(entry("a1", None, "image fetch failed: 503"));
            let a1 = store.image_for("a1").expect("entry exists");
            assert!(
                a1.bitmap.is_some(),
                "a good bitmap survives a transient re-fetch error"
            );
            assert_eq!(store.images.with_untracked(|v| v.len()), 2);

            // A fresh successful decode still replaces (same artifact,
            // same pixels — replacement is harmless and keeps one entry).
            store.upsert_image(entry("a1", bitmap(), ""));
            assert_eq!(store.images.with_untracked(|v| v.len()), 2);

            // An error for an artifact with NO good bitmap does land
            // (the honest failure state renders in the transcript).
            store.upsert_image(entry("a3", None, "decode failed"));
            let a3 = store.image_for("a3").expect("entry exists");
            assert!(a3.bitmap.is_none());
            assert_eq!(a3.error, "decode failed");
        });
        root.dispose();
    }
}
