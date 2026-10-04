// Operator rule (2026-09-30): a persistent on/off setting is a switch labelled by the FEATURE
// (kit `AfSwitch`), never a label that swaps an on/off verb ("Pause"/"Resume", "Turn on"/"Turn
// off", "Enable"/"Disable"). The kit's `findVerbToggleLabels` finds those swaps; this gate runs it
// over every source file of the app. A genuine one-shot action that flips (pausing a running run,
// pausing spoken playback) opts out on its own line with `// state-toggle-lint: allow <reason>`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { findVerbToggleLabels } from "@abstractframework/ui-kit";

const SRC = resolve(__dirname, "..");

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...sourceFiles(path));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) out.push(path);
  }
  return out;
}

describe("state toggles", () => {
  it("scans the app sources (the gate is not empty)", () => {
    const files = sourceFiles(SRC).map((f) => relative(SRC, f));
    expect(files).toContain("workspace/automations_view.tsx");
    expect(files).toContain("workspace/app.tsx");
    expect(files.length).toBeGreaterThan(20);
  });

  it("the guard itself flags a Pause/Resume swap (sanity)", () => {
    expect(findVerbToggleLabels('const l = paused ? "Resume" : "Pause";')).toHaveLength(1);
  });

  it("no source swaps an on/off verb label", () => {
    const hits = sourceFiles(SRC).flatMap((file) =>
      findVerbToggleLabels(readFileSync(file, "utf8")).map((hit) => `${relative(SRC, file)}:${hit.line}: ${hit.text.trim()}`),
    );
    expect(hits).toEqual([]);
  });
});
