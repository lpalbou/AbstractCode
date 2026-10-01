import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AutomationsSection } from "./automations_view";
import {
  ConversationsPanel,
  DEFAULT_PANELS,
  WorkspaceRow,
  workspaceLabel,
  PanelHeader,
  SIDEBAR_PANELS_KEY,
  readPanels,
  togglePanel,
  writePanels,
} from "./sidebar_panels";

const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
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

describe("sidebar panels state", () => {
  it("defaults to both panels open", () => {
    expect(DEFAULT_PANELS).toEqual({ automations: true, conversations: true });
    expect(readPanels(memoryStorage())).toEqual({ automations: true, conversations: true });
  });

  it("toggling flips one panel and persists it under abstractcode.sidebar.panels", () => {
    const store = memoryStorage();
    const next = togglePanel(readPanels(store), "automations");
    expect(next).toEqual({ automations: false, conversations: true });
    writePanels(next, store);
    expect(JSON.parse(store.data[SIDEBAR_PANELS_KEY])).toEqual({ automations: false, conversations: true });
    expect(readPanels(store)).toEqual({ automations: false, conversations: true });
  });

  it("missing or throwing storage gives the defaults and never throws", () => {
    expect(readPanels(null)).toEqual(DEFAULT_PANELS);
    expect(readPanels(undefined)).toEqual(DEFAULT_PANELS);
    const blocked = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceeded");
      },
    };
    expect(readPanels(blocked)).toEqual(DEFAULT_PANELS);
    expect(() => writePanels({ automations: false, conversations: true }, blocked)).not.toThrow();
  });

  it("invalid JSON or wrong shapes give the defaults", () => {
    expect(readPanels(memoryStorage({ [SIDEBAR_PANELS_KEY]: "{not json" }))).toEqual(DEFAULT_PANELS);
    expect(readPanels(memoryStorage({ [SIDEBAR_PANELS_KEY]: "42" }))).toEqual(DEFAULT_PANELS);
    expect(readPanels(memoryStorage({ [SIDEBAR_PANELS_KEY]: '{"automations":"no"}' }))).toEqual(DEFAULT_PANELS);
    expect(readPanels(memoryStorage({ [SIDEBAR_PANELS_KEY]: '{"conversations":false}' }))).toEqual({ automations: true, conversations: false });
  });
});

const automationsBase = {
  state: { items: [], loaded: true, loading: false, listError: null, showArchived: false, selectedId: "" } as any,
  available: { available: true, reason: "" },
  selectedId: "",
  onSelect: () => undefined,
  onRefresh: () => undefined,
  onShowArchived: () => undefined,
};

/** Depth-first search of a React element tree (components are not expanded). */
function findAll(node: unknown, match: (el: React.ReactElement) => boolean, out: React.ReactElement[] = []): React.ReactElement[] {
  if (Array.isArray(node)) node.forEach((n) => findAll(n, match, out));
  else if (React.isValidElement(node)) {
    if (match(node)) out.push(node);
    const props = node.props as Record<string, unknown>;
    findAll(props.children, match, out);
    if (props.actions) findAll(props.actions, match, out);
  }
  return out;
}

describe("sidebar panels DOM", () => {
  it("headers are disclosure buttons with aria-expanded / aria-controls; the region is labelled by its toggle", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...automationsBase} onNew={() => undefined} open onToggle={() => undefined} />);
    expect(html).toMatch(/<button type="button" class="code-panel-toggle" id="code-panel-automations-toggle" aria-expanded="true" aria-controls="code-panel-automations">/);
    expect(html).toMatch(/<div class="code-auto-rows" id="code-panel-automations" role="region" aria-labelledby="code-panel-automations-toggle">/);
    const conv = renderToStaticMarkup(<ConversationsPanel open onToggle={() => undefined}>rows</ConversationsPanel>);
    expect(conv).toMatch(/aria-expanded="true" aria-controls="code-panel-conversations"/);
    expect(conv).toMatch(/<nav class="code-sessions" id="code-panel-conversations" aria-labelledby="code-panel-conversations-toggle">/);
  });

  it("the + and refresh buttons are siblings of the toggle, not inside it", () => {
    const html = renderToStaticMarkup(
      <PanelHeader panel="automations" label="AUTOMATIONS" open onToggle={() => undefined} actions={<button aria-label="New automation">+</button>} />,
    );
    const toggle = /<button type="button" class="code-panel-toggle"[\s\S]*?<\/button>/.exec(html)?.[0] || "";
    expect(toggle).not.toContain("New automation");
    expect(html).toContain('<span class="code-panel-actions"><button aria-label="New automation">');
  });

  it("clicking + or refresh does not toggle the panel", () => {
    const onToggle = vi.fn();
    const onNew = vi.fn();
    const onRefresh = vi.fn();
    const tree = AutomationsSection({ ...automationsBase, onNew, onRefresh, open: true, onToggle });
    const buttons = findAll(tree, (el) => el.type === "button");
    const plus = buttons.find((b) => (b.props as any)["aria-label"] === "New automation")!;
    const refresh = buttons.find((b) => (b.props as any)["aria-label"] === "Refresh automations")!;
    (plus.props as any).onClick();
    (refresh.props as any).onClick();
    expect(onNew).toHaveBeenCalledTimes(1);
    expect(onRefresh).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();
    const header = findAll(tree, (el) => el.type === PanelHeader)[0];
    (header.props as any).onToggle();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("collapsing Automations hides its list; the open Conversations panel is the flex-1 element", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...automationsBase} onNew={() => undefined} open={false} onToggle={() => undefined} />);
    expect(html).toMatch(/data-open="false"/);
    expect(html).toMatch(/aria-expanded="false"/);
    expect(html).toMatch(/<div class="code-auto-rows"[^>]*hidden=""/);
    const conv = renderToStaticMarkup(<ConversationsPanel open onToggle={() => undefined}>rows</ConversationsPanel>);
    expect(conv).toMatch(/<section class="code-panel code-conversations" data-open="true">/);
    expect(css).toMatch(/\.code-panel\[data-open="true"\] \{\s*flex: 1 1 0;\s*min-height: 0;/);
  });

  it("app.tsx renders both panels from the persisted state", () => {
    expect(appSource).toMatch(/useSidebarPanels\(\)/);
    expect(appSource).toMatch(/<ConversationsPanel\s+open=\{panels\.conversations\}/);
    expect(appSource).toMatch(/open=\{panels\.automations\}\s+fill=\{!panels\.conversations\}/);
  });
});

describe("sidebar panels CSS contract", () => {
  it("an open panel's list scrolls on its own; a collapsed panel is header-only", () => {
    expect(css).toMatch(/\.code-panel > nav,\s*\.code-panel > \[role="region"\] \{[^}]*min-height: 0;[^}]*overflow-y: auto;/);
    expect(css).toMatch(/\.code-panel\[data-open="false"\] \{\s*flex: 0 0 auto;/);
    expect(css).toMatch(/\.code-automations\[data-fill="true"\] \{\s*flex: 1 1 0;/);
  });

  it("the toggle is 32 px on desktop and 44 px (--tap-min) under a coarse pointer", () => {
    expect(css).toMatch(/\.code-panel-toggle \{[^}]*min-height: 32px;/);
    expect(css).toMatch(/@media \(pointer: coarse\) \{[\s\S]*?\.code-panel-toggle \{\s*min-height: var\(--tap-min, 44px\);/);
  });
});

describe("Workspace row (round 2, item 10)", () => {
  const long = "/srv/af/runtime/gateway/workspaces/sess_ed8c3b0bf5fd4d249477aca68da7f494-3f2a9c71e0b84d5e";

  it("shows the folder's last segment, the full path as its title, icon | text | chevron", () => {
    const html = renderToStaticMarkup(<WorkspaceRow path={long} onOpen={() => {}} />);
    expect(html).toContain('class="code-workspace-row"');
    expect(html).toContain(`title="Workspace: ${long}"`);
    expect(html).toContain("<small>sess_ed8c3b0bf5fd4d249477aca68da7f494-3f2a9c71e0b84d5e</small>");
    expect(workspaceLabel("")).toBe("Gateway managed");
    expect(appSource).toMatch(/<WorkspaceRow\s+path=\{effectiveWorkspace\}/);
  });

  it("is a three-column grid whose value never wraps or reaches the chevron", () => {
    expect(css).toMatch(/\.code-sidebar-bottom > button\.code-workspace-row \{[^}]*display: grid;[^}]*grid-template-columns: auto minmax\(0, 1fr\) auto;/);
    expect(css).toMatch(/\.code-workspace-row small \{[^}]*min-width: 0;[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap;/);
    expect(css).toMatch(/\.code-sidebar-bottom > button > span \{[^}]*min-width: 0;/);
  });
});

describe("sidebar lists are panels (round 2, item 10)", () => {
  it("each panel has its own kit surface, border, radius and an 8 px inset", () => {
    const rule = css.match(/\.code-panel \{([^}]*)\}/)?.[1] ?? "";
    expect(rule).toMatch(/background: var\(--ui-surface-1\);/);
    expect(rule).toMatch(/border: 1px solid var\(--ui-border-1\);/);
    expect(rule).toMatch(/border-radius: 10px;/);
    expect(rule).toMatch(/margin: 0 8px 8px;/);
  });

  it("the header is the panel's first child (inside the surface)", () => {
    const html = renderToStaticMarkup(
      <ConversationsPanel open onToggle={() => {}}>
        <p>row</p>
      </ConversationsPanel>,
    );
    expect(html).toMatch(/^<section class="code-panel code-conversations"[^>]*><div class="code-section-label">/);
  });
});
