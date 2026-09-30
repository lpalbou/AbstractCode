import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AutomationFolderSection } from "./automations_view";
import {
  DEFAULT_DETAIL_PANELS,
  DETAIL_PANELS_KEY,
  DetailDisclosure,
  placeTimelineSlot,
  readDetailPanels,
  toggleDetailPanel,
  writeDetailPanels,
} from "./detail_panels";

const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
const viewSource = readFileSync(new URL("./automations_view.tsx", import.meta.url), "utf8");
const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

function memoryStorage(initial: Record<string, string> = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => (k in data ? data[k] : null),
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  };
}

/** The body of the block opened by `head` (balanced braces), searched after `from`. */
function block(head: string, from = 0): string {
  const start = css.indexOf(head, from);
  expect(start, `${head} exists`).toBeGreaterThanOrEqual(0);
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(start, i + 1);
  }
  throw new Error("unbalanced");
}
const SPACE = css.indexOf("Space on phones and tablets (DESIGN §12");
/** One rule's declarations inside a block. */
function rule(scope: string, selector: string): string {
  const at = scope.indexOf(`${selector} {`);
  expect(at, `${selector} rule exists`).toBeGreaterThanOrEqual(0);
  return scope.slice(at, scope.indexOf("}", at) + 1);
}

describe("automation detail lists: state", () => {
  it("open by default, both lists", () => {
    expect(DEFAULT_DETAIL_PANELS).toEqual({ occurrences: true, folder: true });
    expect(readDetailPanels(memoryStorage())).toEqual({ occurrences: true, folder: true });
  });

  it("a toggle flips one list and persists it per viewer under abstractcode.automation.panels", () => {
    const store = memoryStorage();
    const next = toggleDetailPanel(readDetailPanels(store), "occurrences");
    expect(next).toEqual({ occurrences: false, folder: true });
    writeDetailPanels(next, store);
    expect(JSON.parse(store.data[DETAIL_PANELS_KEY])).toEqual({ occurrences: false, folder: true });
    expect(readDetailPanels(store)).toEqual({ occurrences: false, folder: true });
  });

  it("blocked, missing or malformed storage gives the defaults and never throws", () => {
    const blocked = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceeded");
      },
    };
    expect(readDetailPanels(blocked)).toEqual(DEFAULT_DETAIL_PANELS);
    expect(readDetailPanels(null)).toEqual(DEFAULT_DETAIL_PANELS);
    expect(readDetailPanels(memoryStorage({ [DETAIL_PANELS_KEY]: "{nope" }))).toEqual(DEFAULT_DETAIL_PANELS);
    expect(readDetailPanels(memoryStorage({ [DETAIL_PANELS_KEY]: '{"folder":"no"}' }))).toEqual(DEFAULT_DETAIL_PANELS);
    expect(() => writeDetailPanels({ occurrences: false, folder: false }, blocked)).not.toThrow();
  });
});

describe("automation detail lists: disclosure markup", () => {
  it("the header is a real disclosure button naming its region", () => {
    const open = renderToStaticMarkup(<DetailDisclosure panel="occurrences" label="Occurrences" count={3} open onToggle={() => {}} />);
    expect(open).toContain('type="button"');
    expect(open).toContain('aria-expanded="true"');
    expect(open).toContain('aria-controls="code-detail-occurrences"');
    expect(open).toContain(">Occurrences<");
    expect(open).toContain(">3<");
    const closed = renderToStaticMarkup(<DetailDisclosure panel="occurrences" label="Occurrences" open={false} onToggle={() => {}} />);
    expect(closed).toContain('aria-expanded="false"');
  });

  it("the folder section: open shows the browser, collapsed keeps only its header", () => {
    const render = (folder: boolean, own = true) =>
      renderToStaticMarkup(
        <AutomationFolderSection panels={{ occurrences: true, folder }} onToggle={() => {}} title="Automation folder" own={own}>
          <section className="pc-ws" aria-label="Automation folder">listing</section>
        </AutomationFolderSection>,
      );
    const open = render(true);
    expect(open).toContain('id="code-detail-folder-toggle"');
    expect(open).toContain('aria-expanded="true"');
    expect(open).toMatch(/<div class="code-detail-region" id="code-detail-folder">/);
    expect(open).toContain('data-folder="automation"');
    const closed = render(false, false);
    expect(closed).toContain('aria-expanded="false"');
    expect(closed).toMatch(/<div class="code-detail-region" id="code-detail-folder" hidden="">/);
    expect(closed).toContain('data-folder="run"');
    // One named region only (the kit browser's), so "Automation folder" stays unambiguous.
    expect(closed).not.toMatch(/code-auto-folder-section"[^>]*aria-label/);
  });

  it("the occurrences header is portalled in front of the kit timeline, and a collapsed list is hidden", () => {
    expect(viewSource).toMatch(/useTimelineSlot\(bodyRef/);
    expect(viewSource).toMatch(/\{timelineSlot\s*\?\s*createPortal\(\s*<DetailDisclosure\s+panel="occurrences"/);
    expect(viewSource).toMatch(/data-occurrences=\{panels\.occurrences \? "open" : "closed"\}/);
    const hide = block('.code-auto-main-body[data-occurrences="closed"] .af-auto__timeline', SPACE);
    expect(hide).toContain('.code-auto-main-body[data-occurrences="closed"] .af-auto__more');
    expect(hide).toMatch(/display: none;/);
  });

  it("disclosure headers are 44 px on touch", () => {
    const coarse = block("@media (pointer: coarse) {", css.indexOf(".code-detail-head {", SPACE));
    expect(coarse).toMatch(/\.code-detail-head \{\s*--code-detail-h: var\(--tap-min, 44px\);/);
    expect(rule(css.slice(SPACE), ".code-detail-toggle")).toContain("min-height: var(--code-detail-h);");
  });
});

/** A minimal DOM for placeTimelineSlot: one parent with ordered children. */
function fakeDom(withMore: boolean, withTimeline = true) {
  const children: any[] = [];
  const parent = {
    children,
    insertBefore(node: any, before: any) {
      const cur = children.indexOf(node);
      if (cur >= 0) children.splice(cur, 1);
      children.splice(children.indexOf(before), 0, node);
      node.parentNode = parent;
      writes++;
    },
  };
  let writes = 0;
  const node = (name: string): any => {
    const n: any = {
      name,
      id: "",
      parentNode: parent,
      remove() {
        children.splice(children.indexOf(n), 1);
        n.parentNode = null;
      },
    };
    Object.defineProperty(n, "nextSibling", { get: () => (n.parentNode ? children[children.indexOf(n) + 1] ?? null : null) });
    return n;
  };
  const head = node("head");
  const more = node("more");
  const timeline = node("timeline");
  children.push(head);
  if (withMore) children.push(more);
  if (withTimeline) children.push(timeline);
  const root = {
    querySelector: (s: string) =>
      s === ".af-auto__timeline" ? (withTimeline ? timeline : null) : s === ".af-auto__more" ? (withMore ? more : null) : null,
  };
  const host = node("host");
  host.parentNode = null;
  return { root, host, timeline, children, writes: () => writes };
}

describe("automation detail lists: the timeline slot", () => {
  it("sits right before the timeline and gives it the region id", () => {
    const d = fakeDom(false);
    expect(placeTimelineSlot(d.root, d.host, "code-detail-occurrences")).toBe(true);
    expect(d.children.map((c) => c.name)).toEqual(["head", "host", "timeline"]);
    expect(d.timeline.id).toBe("code-detail-occurrences");
  });

  it("sits before 'Load earlier occurrences' when the kit shows it", () => {
    const d = fakeDom(true);
    placeTimelineSlot(d.root, d.host, "code-detail-occurrences");
    expect(d.children.map((c) => c.name)).toEqual(["head", "host", "more", "timeline"]);
  });

  it("is idempotent: a second call (a MutationObserver tick) writes nothing", () => {
    const d = fakeDom(false);
    placeTimelineSlot(d.root, d.host, "code-detail-occurrences");
    const before = d.writes();
    placeTimelineSlot(d.root, d.host, "code-detail-occurrences");
    expect(d.writes()).toBe(before);
  });

  it("leaves when the panel shows no timeline", () => {
    const d = fakeDom(false, false);
    d.children.push(d.host);
    d.host.parentNode = { insertBefore() {} };
    expect(placeTimelineSlot(d.root, d.host, "code-detail-occurrences")).toBe(false);
    expect(d.host.parentNode).toBeNull();
  });
});

describe("space on phones and tablets (DESIGN §12)", () => {
  const detail = block("@container code-auto-main (max-width: 779.98px) {", SPACE);
  const phone = block("@media (max-width: 767.98px) {", SPACE);

  it("the detail goes to one column below 780 px of pane width (both columns >= ~360 px above)", () => {
    expect(rule(detail, ".code-auto-main-body")).toContain("grid-template-columns: minmax(0, 1fr);");
    expect(css).not.toContain("@container code-auto-main (max-width: 759.98px)");
  });

  it("label/value facts share a line; a path takes the full row without a box", () => {
    expect(rule(detail, ".code-auto-main-body .af-auto__facts")).toContain("grid-template-columns: max-content minmax(0, 1fr);");
    expect(detail).toMatch(/dd\.af-auto__workspace,[\s\S]*?dd:has\(> code\),[\s\S]*?grid-column: 1 \/ -1;/);
    const path = rule(detail, ".code-auto-main-body .af-auto__path");
    expect(path).toContain("border: 0;");
    expect(path).toContain("background: none;");
  });

  it("no card in the detail: definition, occurrences, transcript turns and the folder are flat", () => {
    for (const sel of [".code-auto-main-body .af-auto__definition", ".code-auto-main-body .af-auto-occ"]) {
      const r = rule(detail, sel);
      expect(r, sel).toContain("border: 0;");
      expect(r, sel).toContain("border-radius: 0;");
    }
    expect(rule(detail, ".code-auto-main-body .af-auto .pc-chat-item")).toContain("padding: 0;");
    expect(detail).toMatch(/\.code-auto-folder-section \.pc-ws \{[^}]*border: 0;/);
    expect(rule(detail, ".code-auto-folder-section .pc-ws__entries")).toContain("overflow: visible;");
  });

  it("reading text is body size in the one-column detail", () => {
    expect(rule(detail, ".code-auto-main-body .af-auto")).toContain("font-size: var(--font-size-body, 14px);");
  });

  it("phones: the approval is flat, the inspector prints the path once at reading size", () => {
    const approval = rule(phone, ".code-conversation .pc-workflow-interaction");
    expect(approval).toContain("border: 0;");
    expect(approval).toContain("padding: 12px 0;");
    expect(rule(phone, ".code-inspector .code-workspace-browser .pc-ws__root")).toContain("display: none;");
    expect(rule(phone, ".code-session-workspace-path code")).toContain("font-size: var(--font-size-body, 14px);");
  });

  it("phones: the navigation drawer is the full-width list screen, rows keep the text within 40 px of the edge", () => {
    expect(rule(phone, ".code-sidebar")).toContain("width: 100vw;");
    expect(rule(phone, ".code-session")).toContain("padding: 10px 8px;");
  });

  it("the inspector backdrop is not a second 'Close workspace inspector' control", () => {
    const at = appSource.indexOf('className="code-inspector-scrim"');
    expect(at).toBeGreaterThan(0);
    const tag = appSource.slice(at, appSource.indexOf("/>", at));
    expect(tag).toContain('aria-hidden="true"');
    expect(tag).not.toContain("aria-label");
  });
});

describe("the navigation drawer scrolls as one (DESIGN §12: no list scrolling inside a scrolling page)", () => {
  const drawer = block("@media (max-width: 1023.98px) {", SPACE);
  it("the drawer is the one scroll; its lists take their content height", () => {
    expect(rule(drawer, ".code-sidebar")).toContain("overflow-y: auto;");
    expect(drawer).toMatch(/\.code-sidebar \.code-panel > \[role="region"\] \{\s*flex: none;\s*overflow: visible;/);
  });
});

describe("type scale floors (DESIGN §12.1)", () => {
  const space = css.slice(SPACE);
  it("desktop: meta 12 px, helper and labels 13 px, at the scale", () => {
    const root = block(":root {", SPACE);
    expect(root).toContain("--font-size-xxs: calc(12px * var(--font-scale));");
    expect(root).toContain("--font-size-xs: calc(13px * var(--font-scale));");
    expect(root).toContain("--font-size-sm: calc(13px * var(--font-scale));");
  });
  it("touch: reading, helper and chrome text 14 px, body 15 px, meta 12 px", () => {
    const coarse = block("@media (pointer: coarse) {", SPACE);
    expect(coarse).toContain("--font-size-xs: calc(14px * var(--font-scale));");
    expect(coarse).toContain("--font-size-sm: calc(14px * var(--font-scale));");
    expect(coarse).toContain("--font-size-base: calc(15px * var(--font-scale));");
    expect(space.indexOf(":root {")).toBeLessThan(space.indexOf(".code-detail-head {"));
  });
  it("paths and sidebar second lines use the xs step, not the 10 px meta step", () => {
    expect(css).toMatch(/\.code-session-workspace-path code \{[^}]*font-size: var\(--font-size-xs\);/);
    expect(css).toMatch(/\.code-session small \{[^}]*font-size: var\(--font-size-xs\);/);
  });
});
