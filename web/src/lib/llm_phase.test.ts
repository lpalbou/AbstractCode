// AbstractCode Web: live LLM phase feedback (prefill vs generation).
//
// The records below are verbatim shapes taken from a hermetic gateway ledger
// (basic-agent@0.0.4 on mlx/Qwen3.5-4B-4bit), trimmed to the fields under test.
import { describe, expect, it } from "vitest";

import { extract_llm_phase_event, format_llm_phase, latest_llm_phase, llm_phase_text } from "./llm_phase";

function progress(payload: Record<string, unknown>, opts: { status?: string } = {}) {
  return {
    run_id: "6ef69750",
    node_id: "reason",
    status: opts.status ?? "completed",
    effect: { type: "emit_event", payload: { name: "abstract.progress", scope: "run", payload } },
    result: { emitted: true, name: "abstract.progress", payload },
  };
}

const PREFILL = {
  kind: "llm",
  phase: "prefill",
  event_index: 0,
  elapsed_s: 0.045,
  provider: "mlx",
  model: "mlx-community/Qwen3.5-4B-4bit",
  prompt_tokens: 5899,
  cached_tokens: 4096,
  fed_tokens: 1803,
  generated_tokens: 0,
};

const GENERATING = {
  kind: "llm",
  phase: "generate",
  event_index: 3,
  elapsed_s: 1.163,
  prompt_tokens: 5899,
  cached_tokens: 4096,
  fed_tokens: 1803,
  generated_tokens: 120,
  tokens_per_second: 45.2,
  ttft_s: 0.153,
};

const COMPLETE = { ...GENERATING, phase: "complete", event_index: 8, generated_tokens: 551, final: true };

describe("extract_llm_phase_event", () => {
  it("reads the payload out of an abstract.progress emit record", () => {
    expect(extract_llm_phase_event(progress(PREFILL))?.phase).toBe("prefill");
  });

  it("ignores generated-media progress on the same event name", () => {
    // Media events carry no `kind` and their own `phase` vocabulary, which
    // already collides ("generate" is a diffusion phase too). Only the
    // discriminator separates them.
    expect(extract_llm_phase_event(progress({ phase: "denoise", step: 4, total_steps: 30, progress: 0.13 }))).toBe(null);
    expect(extract_llm_phase_event(progress({ phase: "generate", step: 4, total_steps: 30, frame: 2 }))).toBe(null);
    expect(extract_llm_phase_event(progress({ phase: "complete", task: "text_to_video", progress: 1 }))).toBe(null);
  });

  it("ignores other emits and non-emit records", () => {
    const status = {
      status: "completed",
      effect: { type: "emit_event", payload: { name: "abstract.status", payload: { value: "Thinking..." } } },
    };
    expect(extract_llm_phase_event(status)).toBe(null);
    expect(extract_llm_phase_event({ status: "started", effect: { type: "llm_call", payload: {} } })).toBe(null);
    expect(extract_llm_phase_event(null)).toBe(null);
  });
});

describe("latest_llm_phase", () => {
  it("returns the newest in-flight event", () => {
    const records = [progress(PREFILL), progress({ ...GENERATING, generated_tokens: 81 }), progress(GENERATING)];
    expect(latest_llm_phase(records)?.generated_tokens).toBe(120);
  });

  it("goes quiet once the call has completed", () => {
    expect(latest_llm_phase([progress(PREFILL), progress(GENERATING), progress(COMPLETE)])).toBe(null);
  });

  it("returns null when the run carries no phase signal at all", () => {
    expect(latest_llm_phase([])).toBe(null);
    expect(
      latest_llm_phase([
        { status: "started", effect: { type: "llm_call", payload: { params: {} } } },
        { status: "completed", effect: { type: "emit_event", payload: { name: "abstract.status", payload: { value: "Thinking..." } } } },
      ]),
    ).toBe(null);
  });
});

describe("format_llm_phase", () => {
  it("renders prefill with the cache split", () => {
    expect(format_llm_phase(PREFILL)).toBe("Prefill · 5,899 tokens (4,096 cached, 1,803 new)");
  });

  it("omits the split when nothing was cached", () => {
    expect(format_llm_phase({ ...PREFILL, cached_tokens: 0, fed_tokens: 5899 })).toBe(
      "Prefill · 5,899 tokens (5,899 new)",
    );
  });

  it("renders live prefill progress next to the label as processed / total (percent)", () => {
    expect(
      format_llm_phase({ kind: "llm", phase: "prefill", prompt_tokens: 5642, prefill_processed_tokens: 2100 }),
    ).toBe("Prefill · 2,100 / 5,642 tokens (37%)");
    // Restored-from-cache tokens are processed at once; the split is not repeated.
    expect(format_llm_phase({ ...PREFILL, prefill_processed_tokens: 4096 })).toBe(
      "Prefill · 4,096 / 5,899 tokens (69%)",
    );
  });

  it("never claims 100% before the first token", () => {
    expect(
      format_llm_phase({ kind: "llm", phase: "prefill", prompt_tokens: 5642, prefill_processed_tokens: 5641 }),
    ).toBe("Prefill · 5,641 / 5,642 tokens (99%)");
    expect(
      format_llm_phase({ kind: "llm", phase: "prefill", prompt_tokens: 5642, prefill_processed_tokens: 5642 }),
    ).toBe("Prefill · 5,642 / 5,642 tokens (99%)");
    expect(
      format_llm_phase({ kind: "llm", phase: "prefill", prompt_tokens: 5642, prefill_processed_tokens: 9000 }),
    ).toBe("Prefill · 5,642 / 5,642 tokens (99%)");
  });

  it("falls back to the total when the provider measured no position (HTTP lanes, one-shot prefill)", () => {
    expect(format_llm_phase({ kind: "llm", phase: "prefill", prompt_tokens: 5642 })).toBe("Prefill · 5,642 tokens");
    // A position without a total is not a progress report.
    expect(format_llm_phase({ kind: "llm", phase: "prefill", prefill_processed_tokens: 2100 })).toBe("Prefill");
  });

  it("falls back to a bare label when the prompt size is unknown", () => {
    expect(format_llm_phase({ kind: "llm", phase: "prefill" })).toBe("Prefill");
  });

  it("renders generation with count and rate", () => {
    expect(format_llm_phase(GENERATING)).toBe("Generating · 120 tokens · 45 tok/s");
  });

  it("singularizes the first token and drops an unknown rate", () => {
    expect(format_llm_phase({ kind: "llm", phase: "generate", generated_tokens: 1 })).toBe("Generating · 1 token");
  });

  it("renders nothing for a completed call or no event", () => {
    expect(format_llm_phase(COMPLETE)).toBe("");
    expect(format_llm_phase(null)).toBe("");
  });
});

describe("llm_phase_text", () => {
  it("is empty without phase records, so callers keep their existing status", () => {
    expect(llm_phase_text([{ status: "started", effect: { type: "llm_call" } }])).toBe("");
  });

  it("updates live as prefill progress events arrive, then switches to generation", () => {
    const start = { ...PREFILL, cached_tokens: 0, fed_tokens: 5642, prompt_tokens: 5642 };
    const seen = [progress(start)];
    expect(llm_phase_text(seen)).toBe("Prefill · 5,642 tokens (5,642 new)");
    seen.push(progress({ ...start, event_index: 1, prefill_processed_tokens: 2100 }));
    expect(llm_phase_text(seen)).toBe("Prefill · 2,100 / 5,642 tokens (37%)");
    seen.push(progress({ ...start, event_index: 2, prefill_processed_tokens: 4352 }));
    expect(llm_phase_text(seen)).toBe("Prefill · 4,352 / 5,642 tokens (77%)");
    seen.push(progress({ ...GENERATING, generated_tokens: 89, tokens_per_second: 34 }));
    expect(llm_phase_text(seen)).toBe("Generating · 89 tokens · 34 tok/s");
  });

  it("tracks the live call end to end", () => {
    const seen = [progress(PREFILL)];
    expect(llm_phase_text(seen)).toBe("Prefill · 5,899 tokens (4,096 cached, 1,803 new)");
    seen.push(progress(GENERATING));
    expect(llm_phase_text(seen)).toBe("Generating · 120 tokens · 45 tok/s");
    seen.push(progress(COMPLETE));
    expect(llm_phase_text(seen)).toBe("");
  });
});
