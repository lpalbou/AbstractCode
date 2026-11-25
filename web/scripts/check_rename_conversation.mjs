// Browser check of renaming a conversation in the real app (its dist/, `npm run build` first) against an in-page
// fake gateway (Playwright routes; no gateway, no model). Optional, like check_session_cards.mjs:
//   node scripts/check_rename_conversation.mjs
import assert from "node:assert/strict";
import { chromium } from "@playwright/test";
import { createCodeServer } from "../bin/server.js";

const server = createCodeServer();
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}/`;
const at = (d) => new Date(Date.now() - d * 3600000).toISOString();
// The fake gateway's state.
const gw = { sessionTitles: true, titles: { s3: "Alpha" }, puts: [], hold: null, failNext: null, archived: new Set(), titlesError: null };
const sessions = [
  ["s1", "Write a snake game in Python", 1],
  ["s2", "A conversation name that is long enough to be clamped to two lines in the sidebar so the tooltip shows it whole", 2],
  ["s3", "Plan the release", 3],
  ["s4", "Budget meeting notes", 4],
];
const row = ([id, prompt, age]) => ({ run_id: `run-${id}`, session_id: id, session_kind: "chat", status: "completed", parent_run_id: null, created_at: at(age), updated_at: at(age), tool_calls: 0, input_data: { prompt }, ...(gw.titles[id] ? { session_title: gw.titles[id] } : {}) });
let browser;
const ok = [];
const pass = (name) => { ok.push(name); };
try {
  browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await context.route("**/api/**", async (route) => {
    const req = route.request();
    const u = new URL(req.url());
    const path = u.pathname.replace(/^\/api\//, "");
    const json = (body, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "connection/gateway") return json({ ok: true, has_session: true, gateway_url: "http://fake", gateway: { ok: true, principal: { user_id: "demo" } } });
    if (path === "gateway/me") return json({ ok: true, principal: { user_id: "demo" } });
    if (path === "gateway/discovery/capabilities") return json({ capabilities: { ...(gw.sessionTitles ? { session_titles: true } : {}), contracts: {} } });
    if (path === "gateway/runs") {
      const archivedOnly = u.searchParams.get("archived_only") === "true";
      const items = sessions.filter(([id]) => gw.archived.has(id) === archivedOnly).map(row);
      return json({ items, has_more: false, archived_sessions: gw.archived.size, ...(gw.titlesError && !archivedOnly ? { session_titles_error: gw.titlesError } : {}) });
    }
    const m = /^gateway\/sessions\/([^/]+)\/title$/.exec(path);
    if (m && req.method() === "PUT") {
      const id = decodeURIComponent(m[1]);
      const title = JSON.parse(req.postData() || "{}").title;
      gw.puts.push([id, title]);
      if (gw.hold) await gw.hold;
      if (gw.failNext) { const f = gw.failNext; gw.failNext = null; return json({ detail: { reason_code: f, message: "The titles file cannot be read." } }, 500); }
      gw.titles[id] = title || undefined;
      return json({ session_id: id, title: title || null, changed: true });
    }
    if (/\/input_data$/.test(path)) return json({ input_data: {} });
    if (/\/stream$/.test(path)) return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    if (path === "gateway/me/events") return json({ events: [], head: 0, next_after: 0, more: false });
    if (path === "gateway/accounts/me/preferences") return json({ preferences: {} });
    if (path === "gateway/bundles") return json({ items: [], default_agent_workflows: {}, default_agent_workflows_unavailable: {} });
    return json({ items: [], has_more: false });
  });
  await page.goto(base);
  const card = (id) => page.locator(`.code-session-item[data-item-id="${id}"]`);
  const input = page.locator(".code-session-item input[aria-label='Rename conversation']");
  await card("s1").locator(".af-session-card__rename").waitFor({ state: "attached" });

  // Hover: Rename and Archive are both hittable, side by side, and the row's text makes room for them.
  await card("s2").hover();
  const boxes = await card("s2").evaluate((el) => {
    const r = (s) => el.querySelector(s).getBoundingClientRect();
    const open = el.querySelector(".af-session-card__open");
    const rename = r(".af-session-card__rename"), archive = r(".af-session-card__archive"), title = r(".af-session-card__title"), meta = r(".af-session-card__meta");
    const hit = (b) => document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2)?.closest("button")?.className;
    return { rename, archive, titleRight: title.right, metaRight: meta.right, hitRename: hit(rename), hitArchive: hit(archive), pad: parseFloat(getComputedStyle(open).paddingRight) };
  });
  assert.equal(boxes.hitRename, "af-session-card__rename");
  assert.equal(boxes.hitArchive, "af-session-card__archive");
  assert.ok(boxes.rename.right <= boxes.archive.left, "Rename left of Archive, not under it");
  assert.ok(boxes.titleRight <= boxes.rename.left && boxes.metaRight <= boxes.rename.left, "the title's end and the details never run under the actions");
  pass("hover: both actions hittable, text makes room");

  // F2 opens the editor; Escape restores the tooltip the clamped title had (every time).
  await card("s2").locator(".af-session-card__open").focus();
  const tipBefore = await card("s2").locator(".af-session-card__open").getAttribute("data-af-tip");
  assert.ok(tipBefore, "a clamped name has a tooltip");
  for (let i = 0; i < 6; i++) {
    await card("s2").locator(".af-session-card__open").focus();
    await page.keyboard.press("F2");
    await input.waitFor();
    await page.keyboard.press("Escape");
    await card("s2").locator(".af-session-card__open").waitFor();
    assert.equal(await card("s2").locator(".af-session-card__open").getAttribute("data-af-tip"), tipBefore, `tooltip back at once after editing (${i})`);
  }
  const tipAfter = await card("s2").locator(".af-session-card__open").getAttribute("data-af-tip");
  assert.equal(tipAfter, tipBefore, "tooltip back after editing: " + JSON.stringify([tipAfter, await card("s2").locator(".af-session-card__title").getAttribute("title")]));
  assert.equal(await card("s2").locator(".af-session-card__title").getAttribute("title"), null);
  pass("tooltip restored");

  // A refusal: the error under the row describes the input, and goes when the editor closes.
  gw.failNext = "session_titles_unwritable";
  await card("s1").locator(".af-session-card__open").focus();
  await page.keyboard.press("F2");
  await input.fill("Snake");
  await input.press("Enter");
  const alert = card("s1").locator("[role='alert']");
  await alert.waitFor();
  assert.equal(await alert.textContent(), "Not renamed: The titles file cannot be read.");
  assert.equal(await page.locator(".code-session-item [role='alert']").count(), 1, "the error is under that row only");
  assert.equal(await input.getAttribute("aria-invalid"), "true");
  assert.ok((await input.getAttribute("aria-describedby")).split(" ").includes(await alert.getAttribute("id")));
  assert.equal(await input.inputValue(), "Snake");
  await input.press("Escape");
  await alert.waitFor({ state: "detached" });
  pass("refusal described, cleared on Escape");

  // Searching: the row being renamed stays while its save is pending, whatever it no longer matches.
  const search = page.getByLabel("Search conversations", { exact: true });
  await search.fill("alpha");
  await card("s3").waitFor();
  let release;
  gw.hold = new Promise((r) => { release = r; });
  await card("s3").locator(".af-session-card__open").focus();
  await page.keyboard.press("F2");
  await input.fill("Quarterly budget");
  await input.press("Enter");
  await page.waitForTimeout(200);
  assert.equal(await card("s3").count(), 1, "kept while pending");
  assert.equal(await input.evaluate((el) => el.readOnly), true);
  gw.hold = null;
  release();
  await page.waitForTimeout(300);
  assert.equal(await card("s3").count(), 0, "gone once saved (its new name and first message do not match)");
  await search.fill("");
  await card("s3").locator(".af-session-card__title", { hasText: "Quarterly budget" }).waitFor();
  pass("search keeps the row being renamed");

  // The open conversation clicked again keeps the draft.
  await card("s4").locator(".af-session-card__open").click();
  const composer = page.locator(".code-conversation textarea").first();
  await composer.waitFor();
  await composer.fill("A draft I am still writing");
  await card("s4").locator(".af-session-card__open").click();
  await page.waitForTimeout(200);
  assert.equal(await composer.inputValue(), "A draft I am still writing");
  pass("re-opening the open conversation keeps the draft");

  // Names that cannot be read: one notice in the sidebar, gone after a list without the error; rename still offered.
  gw.titlesError = { reason_code: "session_titles_unreadable", message: "The titles file cannot be read." };
  await page.getByRole("button", { name: "Refresh conversations" }).click();
  const notice = page.locator('[data-notice="session-titles"]');
  await notice.waitFor();
  assert.equal(await notice.textContent(), "Conversation names could not be loaded: The titles file cannot be read.");
  assert.ok(await card("s1").locator(".af-session-card__rename").count());
  gw.titlesError = null;
  await page.getByRole("button", { name: "Refresh conversations" }).click();
  await notice.waitFor({ state: "detached" });
  pass("unreadable names notice");

  // An older gateway (no session_titles): no Rename anywhere, F2 does nothing, nothing is sent.
  gw.sessionTitles = false;
  const puts = gw.puts.length;
  await page.reload();
  await card("s1").waitFor();
  await page.waitForTimeout(500);
  assert.equal(await page.locator(".af-session-card__rename").count(), 0);
  await card("s1").locator(".af-session-card__open").focus();
  await page.keyboard.press("F2");
  await page.waitForTimeout(200);
  assert.equal(await input.count(), 0);
  assert.equal(gw.puts.length, puts);
  pass("older gateway: no rename, no request");

  assert.deepEqual(errors, []);
  console.log(`check_rename_conversation: PASS (${ok.join(", ")})`);
} finally {
  await browser?.close();
  server.closeAllConnections?.();
  server.close();
}
