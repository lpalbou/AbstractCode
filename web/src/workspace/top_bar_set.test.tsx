// The shared top-bar set (operator ruling 2026-10-29: "change the widget once,
// it changes everywhere"): the docs drawer button, the theme picker
// (Appearance), the About dialog and the account widget are the KIT's
// components, never local copies. Code passes all four to the kit's
// AfTopBarActions: the docs toggle opens panel-chat's docs panel
// (codeDocsPanel), `appearance` {value, onChange} makes the kit render its
// AfAppearanceDialog, `about` its AfAboutDialog, and identity="menu" with the
// useGatewayConnection value its ONE account widget. Removing a piece from
// app.tsx (or re-implementing one locally) turns this red.
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AfTopBarActions, appIdentity } from "@abstractframework/ui-kit";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const assistantSource = readFileSync(new URL("./app_assistant.tsx", import.meta.url), "utf8");

/** The props text of the first `<Tag ... />` in `src` (brace-aware: `/>` inside `{...}` does not end it). */
function jsxProps(src: string, tag: string): string {
  const start = src.indexOf(`<${tag}`);
  expect(start, `<${tag} in app.tsx`).toBeGreaterThanOrEqual(0);
  let depth = 0;
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (depth === 0 && c === "/" && src[i + 1] === ">") return src.slice(start, i);
  }
  throw new Error(`<${tag} is never closed`);
}

describe("the shared top-bar set", () => {
  const bar = jsxProps(appSource, "AfTopBarActions");

  it("passes the four kit pieces to AfTopBarActions", () => {
    expect(bar).toMatch(/\bdocs=\{\{ open: railPanel === "docs",/);
    // {value, onChange}: the kit owns and renders AfAppearanceDialog.
    expect(bar).toMatch(/\bappearance=\{\{ value: appearance, onChange: setAppearance \}\}/);
    expect(bar).toMatch(/\babout=\{\{ identity: APP_IDENTITY,/);
    expect(bar).toContain('identity="menu"');
    // The hook's value: the account reads connection.status, its Disconnect calls connection.signOut().
    expect(bar).toMatch(/\bconnection=\{connection\}/);
    expect(appSource).toMatch(/const connection = useGatewayConnection\(/);
  });

  it("the docs drawer is panel-chat's, and no piece is a local copy", () => {
    expect(assistantSource).toMatch(/import \{ docsAssistantPanel[^}]*\} from "@abstractframework\/panel-chat"/);
    expect(appSource).toMatch(/docs=\{codeDocsPanel\(connection\.connected\)\}/);
    for (const local of [/function \w*About\w*Dialog/, /function \w*Appearance\w*Dialog/, /Signed in as/, /af-topbar__/]) expect(appSource).not.toMatch(local);
  });

  it("the installed kit renders the set with the one account widget (ui-kit ^0.9.6)", () => {
    const status = { ok: true, has_session: true, gateway_url: "http://gw.local:8080", gateway: { ok: true, principal: { user_id: "alice" } } };
    const html = renderToStaticMarkup(
      <AfTopBarActions
        docs={{ open: false, onToggle: () => {} }}
        appearance={{ value: { theme: "dark", font_scale: "md", header_density: "comfortable" } as any, onChange: () => {} }}
        about={{ identity: appIdentity("abstractcode", "0.0.0") }}
        identity="menu"
        connection={{ phase: "connected", onConnect: () => {}, onDisconnect: () => {}, status }}
      />,
    );
    const at = ["af-topbar__btn--docs", "af-topbar__btn--appearance", "af-topbar__btn--about", 'data-af-identity="menu"'].map((m) => html.indexOf(m));
    expect(at.every((v, i) => v >= 0 && (i === 0 || v > at[i - 1]))).toBe(true);
    expect(html).toContain('<span class="af-topbar__identity-label">alice</span>');
    expect(html).not.toContain("af-topbar__pill");
  });
});
