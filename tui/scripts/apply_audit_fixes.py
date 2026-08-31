#!/usr/bin/env python3
"""Apply the adversarial audit's four blockers (B1-B4) plus two lesser fixes.

Held as a script because a benchmark matrix was running when the audit landed:
the harness's stray-write detector diffs `git status --porcelain` and watches
`src/`, so editing the Rust tree mid-run would have been attributed to the agent
and falsely discarded a run — the exact mistake that voided an earlier matrix.

Run AFTER the matrix completes:  python3 scripts/apply_audit_fixes.py
Then: cargo fmt && cargo test --release
"""
from __future__ import annotations

import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]


def patch(rel: str, old: str, new: str, label: str) -> None:
    p = REPO / rel
    s = p.read_text()
    n = s.count(old)
    if n != 1:
        print(f"  ✗ {label}: anchor matched {n}× in {rel} (expected 1) — SKIPPED", file=sys.stderr)
        return
    p.write_text(s.replace(old, new, 1))
    print(f"  ✓ {label}  ({rel})")


# ---------------------------------------------------------------- B1
# `budget_exhausted` was set on a truncated turn and never cleared, so EVERY
# later turn in the session rendered "⚠ stopped: iteration budget (N)" — the
# fix against claiming false success started claiming false failure instead.
patch(
    "src/transcript.rs",
    """        self.finished = false;
        self.failed = false;
        self.activity.clear();""",
    """        self.finished = false;
        self.failed = false;
        // Per-TURN verdict, cleared like `failed`. Leaving it latched made a
        // truncated turn poison every later turn in the session: turn 2 of a
        // clean run still rendered "stopped: iteration budget (N)" in the
        // fixed chrome. A honesty fix that lies in the other direction is
        // still a lying client.
        self.budget_exhausted = None;
        self.activity.clear();""",
    "B1 budget_exhausted no longer sticky",
)

# ---------------------------------------------------------------- B2
# `/workflow` re-loads the catalog from PREFS, but the substitution check
# compared against the launch-time --workflow flag, which is never cleared. So
# opening the picker posted a false "not found on this gateway" error card for
# a workflow that is installed — and nothing had actually changed.
patch(
    "src/runner.rs",
    """                let substitution = self.requested_workflow.as_deref().and_then(|raw| {""",
    """                // BOOT LOAD ONLY. `/workflow` re-issues LoadCatalog with the
                // SAVED PREFS, not the CLI flag, so evaluating the flag again
                // on every picker open posted a false "not found" card for an
                // installed workflow, repeatedly, while `store.workflow` was
                // left untouched anyway.
                let first_load = !self.catalog_loaded;
                let substitution = self
                    .requested_workflow
                    .as_deref()
                    .filter(|_| first_load)
                    .and_then(|raw| {""",
    "B2 substitution check is boot-only",
)

# ---------------------------------------------------------------- B3
# The installed-but-wrong-interface branch required an explicit flow half, so a
# bundle-only ref to an installed non-agent bundle (`coder`, `goal-agent`,
# `docs-qa`, …) still got the flat "not found on this gateway" lie.
patch(
    "src/exec.rs",
    """    // Ambiguous bundle-only ref: the bundle holds several agent flows and
    // none is named after it, so no flow is the operator's evident intent.
    if f.is_none() {""",
    """    // Bundle-only ref naming an INSTALLED bundle that simply has no agent
    // entrypoint. Without this the commonest spelling (`--workflow coder`)
    // reported "not found on this gateway" for a bundle sitting right there —
    // the same lie the diagnosed refusal exists to end, just one input shape
    // further along.
    if f.is_none() && crate::discovery::flows_in_bundle(available, &b).is_empty() {
        let eps: Vec<&(String, String, Vec<String>)> =
            catalog.iter().filter(|(cb, _, _)| cb == &b).collect();
        if !eps.is_empty() {
            let mut msg = format!(
                "✗ bundle '{b}' is installed but has no '{iface}' entrypoint — \
                 refusing to run a different agent\\n  its entrypoints:",
                iface = crate::discovery::AGENT_INTERFACE_V1
            );
            for (_, flow, ifs) in eps {
                let i = if ifs.is_empty() {
                    "none declared".to_string()
                } else {
                    ifs.join(", ")
                };
                msg.push_str(&format!("\\n    {b}:{flow}  ({i})"));
            }
            return Some(msg);
        }
    }
    // Ambiguous bundle-only ref: the bundle holds several agent flows and
    // none is named after it, so no flow is the operator's evident intent.
    if f.is_none() {""",
    "B3 bundle-only refs name the real reason",
)

# ---------------------------------------------------------------- lesser 2
# ADR-0027 §4 says EVERY timeout site carries the marker; this one did not, so
# the "all sites tagged" claim was false as written.
patch(
    "src/ui/quit.rs",
    """pub(crate) const QUIT_ACK_TIMEOUT: Duration = Duration::from_secs(8);""",
    """/// `#[WARNING:TIMEOUT]` quit-acknowledgement grace (ADR-0027 §4).
/// NOT a run deadline: the run keeps going on the gateway and a late ack is
/// still honored (`quit.rs` late-ack path). It only bounds how long the UI
/// waits before letting the operator go.
pub(crate) const QUIT_ACK_TIMEOUT: Duration = Duration::from_secs(8);""",
    "lesser-2 QUIT_ACK_TIMEOUT tagged",
)

# ---------------------------------------------------------------- lesser 5
# `iterations` from the runtime is the count USED, not the ceiling.
patch(
    "src/transcript.rs",
    """                if iters > 0 {
                    format!("stopped: iteration budget ({iters})")""",
    """                if iters > 0 {
                    // `iterations` is the count USED (react_runtime sets it from
                    // `current_iteration`), not the ceiling — "budget (7)" read
                    // as if 7 were the limit.
                    format!("stopped: iteration budget after {iters} iterations")""",
    "lesser-5 budget wording names iterations used",
)

print("\nNext: cargo fmt && cargo test --release")
print("Still TODO by hand (needs judgement, not a patch):")
print("  B4  — gate the project-context / verifier notices on native-loop bundles")
print("  L1  — --help says max-iterations default 50 while the runtime seeds 20")
print("  L3  — /goal tool injection races the async tools load")
print("  L7  — orphaned doc comment above workflow_is_review_capable (discovery.rs)")
