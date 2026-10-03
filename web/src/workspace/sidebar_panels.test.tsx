import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AutomationsSection } from "./automations_view";
import {
  ConversationsPanel,
  DEFAULT_PANELS,
  SidebarLists,
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

  it("collapsing Automations hides its list (the header stays)", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...automationsBase} onNew={() => undefined} open={false} onToggle={() => undefined} />);
    expect(html).toMatch(/data-open="false"/);
    expect(html).toMatch(/aria-expanded="false"/);
    expect(html).toMatch(/<div class="code-panel-header"><button/);
    expect(html).toMatch(/<div class="code-auto-rows"[^>]*hidden=""/);
  });

  it("app.tsx renders both drawers from the persisted state inside the drawers frame, the bottom block after it", () => {
    expect(appSource).toMatch(/useSidebarPanels\(\)/);
    expect(appSource).toMatch(/<ConversationsPanel\s+open=\{panels\.conversations\}/);
    expect(appSource).toMatch(/open=\{panels\.automations\}\s+onToggle=/);
    const lists = /<SidebarLists panels=\{panels\}>([\s\S]*?)<\/SidebarLists>\s*<div className="code-sidebar-bottom">/.exec(appSource)?.[1] ?? "";
    expect(lists).toMatch(/^\s*<AutomationsSection[\s\S]*<ConversationsPanel[\s\S]*<\/ConversationsPanel>\s*$/);
    expect(renderToStaticMarkup(<SidebarLists panels={{ automations: true, conversations: false }}>x</SidebarLists>)).toBe(
      '<div class="code-sidebar-lists" data-automations="open" data-conversations="closed">x</div>',
    );
    expect(renderToStaticMarkup(<SidebarLists panels={{ automations: false, conversations: true }}>x</SidebarLists>)).toContain(
      'data-automations="closed" data-conversations="open"',
    );
  });
});

/** One rule's declarations (the first `selector {` at top level of the stylesheet). */
function cssRule(selector: string): string {
  const at = css.indexOf(`\n${selector} {`);
  expect(at, `${selector} rule exists`).toBeGreaterThanOrEqual(0);
  return css.slice(at, css.indexOf("}", at) + 1);
}
const decl = (rule: string, prop: string) => new RegExp(`\\n\\s*${prop}: ([^;]+);`).exec(rule)?.[1];

describe("panel headers (round 3: the operator's 12:00 screenshot of 0.9.0)", () => {
  it("each header is a full-width row: the toggle (chevron, then label) on the left, the actions on the right", () => {
    const html = renderToStaticMarkup(
      <AutomationsSection {...automationsBase} onNew={() => undefined} open onToggle={() => undefined} />,
    );
    const header = /<div class="code-panel-header">([\s\S]*?)<\/div><div class="code-auto-rows"/.exec(html)?.[1] ?? "";
    expect(header).toMatch(/^<button type="button" class="code-panel-toggle"[^>]*aria-expanded="true"[^>]*><svg[\s\S]*?<\/svg><span>Automations<\/span><\/button><span class="code-panel-actions">/);
    expect(header).toMatch(/<span class="code-panel-actions">[\s\S]*aria-label="New automation"[\s\S]*aria-label="Refresh automations"[\s\S]*<\/span>$/);
    const conv = renderToStaticMarkup(
      <ConversationsPanel open={false} onToggle={() => {}} actions={<button aria-label="Refresh conversations">r</button>}>
        <p>row</p>
      </ConversationsPanel>,
    );
    expect(conv).toMatch(/^<section class="code-panel code-conversations" data-open="false"><div class="code-panel-header"><button type="button" class="code-panel-toggle"[^>]*aria-expanded="false"[^>]*><svg[\s\S]*?<\/svg><span>Conversations<\/span><\/button><span class="code-panel-actions"><button aria-label="Refresh conversations">/);
  });

  it("the header row is a raised 44 px row (surface-2 background, border-2, 8 px radius)", () => {
    const header = cssRule(".code-panel-header");
    expect(decl(header, "display")).toBe("flex");
    expect(decl(header, "background")).toBe("var(--ui-surface-2)");
    expect(decl(header, "border")).toBe("1px solid var(--ui-border-2)");
    expect(decl(header, "border-radius")).toBe("8px");
    expect(decl(header, "min-height")).toBe("var(--tap-min, 44px)");
    // The toggle takes the row's free width, so the actions sit at the right edge.
    expect(decl(cssRule(".code-panel-toggle"), "flex")).toBe("1 1 auto");
    expect(decl(cssRule(".code-panel-actions"), "flex")).toBe("none");
  });

  it("the items sit on the plain sidebar background: the panel is no card", () => {
    const panel = cssRule(".code-panel");
    for (const prop of ["background", "border", "border-radius", "overflow", "max-height", "height"]) {
      expect(decl(panel, prop), `.code-panel ${prop}`).toBeUndefined();
    }
  });
});

describe("two stacking drawers (round 4, DESIGN §3): fold/split, each list scrolls inside its drawer", () => {
  /** The declarations of a rule whose selector list is exactly `selectors` (one per line). */
  const ruleFor = (selectors: string) => {
    const at = css.indexOf(`\n${selectors} {`);
    expect(at, `${selectors} rule exists`).toBeGreaterThanOrEqual(0);
    return css.slice(at, css.indexOf("}", at) + 1);
  };

  it("the frame takes the free height and never scrolls itself; a closed drawer is its header only", () => {
    const lists = cssRule(".code-sidebar-lists");
    expect(decl(lists, "display")).toBe("flex");
    expect(decl(lists, "flex-direction")).toBe("column");
    expect(decl(lists, "flex")).toBe("1 1 0");
    expect(decl(lists, "min-height")).toBe("0");
    expect(decl(lists, "overflow")).toBe("hidden");
    const panel = cssRule(".code-panel");
    expect(decl(panel, "flex")).toBe("none");
    expect(decl(panel, "min-height")).toBe("0");
  });

  it("Automations open: both drawers share the height (the Conversations header mid-height); only Conversations open: it takes the rest", () => {
    const split = ruleFor(
      '.code-sidebar-lists[data-automations="open"] > .code-panel,\n.code-sidebar-lists[data-automations="closed"][data-conversations="open"] > .code-conversations',
    );
    expect(decl(split, "flex")).toBe("1 1 0");
    // Nothing else grows a drawer: both closed = the two header rows at the top.
    expect(css.match(/> \.code-(panel|conversations|automations)[^{]*\{[^}]*flex: 1 1 0/g) ?? []).toHaveLength(1);
  });

  it("each list scrolls inside its drawer (never clipped under the other header)", () => {
    const list = ruleFor('.code-panel > nav,\n.code-panel > [role="region"]');
    expect(decl(list, "flex")).toBe("1 1 auto");
    expect(decl(list, "min-height")).toBe("0");
    expect(decl(list, "overflow-y")).toBe("auto");
  });

  it("a short phone landscape scrolls the whole drawer instead (each part at its content height)", () => {
    const short = css.slice(css.indexOf("@media (max-height: 500px) {"));
    expect(short).toMatch(/\.code-sidebar \.code-sidebar-lists,\n\s*\.code-sidebar \.code-sidebar-lists > \.code-panel,\n\s*\.code-sidebar \.code-panel > nav,\n\s*\.code-sidebar \.code-panel > \[role="region"\] \{\n\s*flex: none;\n\s*overflow: visible;/);
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
    expect(appSource).toContain('className="code-current-workspace"');
    expect(appSource).toContain('{effectiveWorkspace || "Gateway workspace"}');
  });

  it("is a three-column grid whose value never wraps or reaches the chevron", () => {
    expect(css).toMatch(/\.code-sidebar-bottom > button\.code-workspace-row \{[^}]*display: grid;[^}]*grid-template-columns: auto minmax\(0, 1fr\) auto;/);
    expect(css).toMatch(/\.code-workspace-row small \{[^}]*min-width: 0;[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap;/);
    expect(css).toMatch(/\.code-sidebar-bottom > button > span \{[^}]*min-width: 0;/);
  });
});



describe("automation sidebar background refresh", () => {
  it("keeps the empty state in place without inserting a loading row after the first load", () => {
    const render = (loading: boolean, loaded = true) => renderToStaticMarkup(<AutomationsSection {...automationsBase}
      state={{ ...automationsBase.state, items: [], loaded, loading }} />);
    expect(render(true)).not.toContain("Loading automations");
    expect(render(true)).toContain("No automations yet");
    expect(render(false)).toContain("No automations yet");
    expect(render(true, false)).toContain("Loading automations");
  });
});
