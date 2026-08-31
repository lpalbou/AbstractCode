import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// This app must build from its own directory alone, resolving every shared
// dependency through node_modules like any other package.
//
// It did not always. The four @abstractframework/* kit packages were once
// wired up as Vite aliases and tsconfig `paths` pointing at `../../abstractuic`
// — a sibling checkout OUTSIDE the repo. Three things followed, and all three
// were live defects rather than hypotheticals:
//
//   1. The app built only where a matching sibling happened to sit, so `web/`
//      could not be moved or the repo re-cloned elsewhere.
//   2. CI checked that sibling out with no `ref:`, so every build floated on
//      whatever was on that repo's default branch that day.
//   3. The kit moved a stylesheet from a component self-import to an explicit
//      host import; against the sibling checkout the app silently lost those
//      styles, because nothing declared the dependency that would have caught it.
//
// These tests fail the build if any of that comes back.

const web_root = fileURLToPath(new URL("../..", import.meta.url));

// This file names the very specifiers and paths it forbids, inside regexes and
// prose. Scanning it would report its own assertions as violations.
const SELF = "dependency_hygiene.test.ts";

function source_files(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "dist" || entry === SELF) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) source_files(full, out);
    else if (/\.tsx?$/.test(entry)) out.push(full);
  }
  return out;
}

function read(relative: string): string {
  return readFileSync(join(web_root, relative), "utf8");
}

// Deliberately NOT done by stripping comments first. A regex comment-stripper
// is wrong for this language: `"src/**/*.test.ts"` in the vitest config both
// opens and closes a block comment as far as a regex can tell, so stripping
// silently deletes real code between two globs — and a guard that deletes the
// code it inspects passes while the defect it guards is live. (It did.)
//
// Instead, match only inside DOUBLE- or SINGLE-quoted strings. Every alias and
// `paths` entry is a quoted string; prose in this repo's comments refers to
// such paths in `backticks`, which these patterns do not match.
const QUOTED_ABSTRACTUIC_PATH = /["'][^"'\n]*\.\.\/[^"'\n]*abstractuic[^"'\n]*["']/i;
const QUOTED_KIT_ALIAS_KEY = /["']@abstractframework\/[^"'\n]*["']\s*:/;

/**
 * Every `@abstractframework/...` specifier imported anywhere under src/.
 *
 * Only module-specifier POSITIONS count — `from "x"`, `import "x"`,
 * `import("x")`, `require("x")`, `.resolve("x")`. A package name that merely
 * appears in a string (a `describe()` title, a message) is not an import, and
 * counting it would demand a dependency nothing actually loads.
 */
function imported_kit_specifiers(): Set<string> {
  const found = new Set<string>();
  const specifier = /(?:\bfrom|\bimport|\brequire|\.resolve)\s*\(?\s*["'](@abstractframework\/[^"'\s]+)["']/g;
  for (const file of source_files(join(web_root, "src"))) {
    for (const match of readFileSync(file, "utf8").matchAll(specifier)) {
      found.add(match[1]);
    }
  }
  return found;
}

/** "@scope/name/sub/path.css" -> "@scope/name" */
function package_of(specifier: string): string {
  const [scope, name] = specifier.split("/");
  return `${scope}/${name}`;
}

describe("web builds from its own directory", () => {
  it("declares every shared kit package it imports", () => {
    const manifest = JSON.parse(read("package.json"));
    const declared = new Set(Object.keys(manifest.dependencies || {}));

    const imported = [...imported_kit_specifiers()].map(package_of);
    expect(imported.length).toBeGreaterThan(0); // guard against a silent no-op

    for (const pkg of new Set(imported)) {
      expect(declared, `${pkg} is imported but not in package.json dependencies`).toContain(pkg);
    }
  });

  it("resolves every imported kit specifier from node_modules", async () => {
    // The real proof: ask Node to resolve exactly what the source imports,
    // including subpaths like `.../agent_cycles.css`, through the package's
    // own `exports` map. A stale alias would hide a missing subpath export.
    const { createRequire } = await import("node:module");
    const require_ = createRequire(join(web_root, "package.json"));

    for (const specifier of imported_kit_specifiers()) {
      expect(() => require_.resolve(specifier), `cannot resolve ${specifier}`).not.toThrow();
    }
  });

  it("keeps the build config free of sibling-checkout paths", () => {
    for (const config of ["vite.config.ts", "tsconfig.json", "tsconfig.node.json", "package.json"]) {
      expect(read(config), `${config} points at a sibling abstractuic checkout`).not.toMatch(
        QUOTED_ABSTRACTUIC_PATH,
      );
    }
  });

  it("does not alias the kit packages away from node_modules", () => {
    // An alias/paths entry is `"@abstractframework/x": [...]` or `: resolve(...)`.
    expect(read("vite.config.ts"), "vite.config.ts aliases a kit package").not.toMatch(QUOTED_KIT_ALIAS_KEY);
    expect(read("tsconfig.json"), "tsconfig.json remaps a kit package").not.toMatch(QUOTED_KIT_ALIAS_KEY);
  });

  it("imports the kit stylesheets its components need", () => {
    // monitor-flow's AgentCyclesPanel dropped its own `import "./agent_cycles.css"`
    // once every host imported it explicitly. Rendering the panel without this
    // import produces an unstyled panel and no error anywhere — exactly the
    // regression that shipped before.
    const app = read("src/ui/app.tsx");
    if (app.includes("AgentCyclesPanel")) {
      expect(app, "AgentCyclesPanel is rendered without its stylesheet").toContain(
        "@abstractframework/monitor-flow/agent_cycles.css",
      );
    }
  });
});
