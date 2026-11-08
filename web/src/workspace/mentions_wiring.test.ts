// How Code mounts the mention rules (`mentions.ts`): what reaches the shared chat, and in which
// order a send with picked names runs. The rules themselves are checked in mentions.test.ts.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const chatMount = app.slice(app.indexOf("<WorkflowChat"), app.indexOf("onCancel={cancelCurrentRun}"));

describe("Code's chat offers @ names from the gateway", () => {
  it("the shared chat gets the names, the first-pick sentence, the conversation key and both sentences", () => {
    expect(chatMount).toContain("onSend={send}");
    expect(chatMount).toContain("mentionCandidates={mentionCandidates}");
    expect(chatMount).toContain("mentionNotice={mentionNoticeFor}");
    expect(chatMount).toContain("mentionSessionKey={`${identity}:${session.sessionId}`}");
    expect(chatMount).toContain("mentionsUnavailable={mentionsOff === identity ? MENTIONS_UNAVAILABLE : null}");
    expect(chatMount).toContain("mentionStatus={mentionStatus?.sessionId === session.sessionId ? mentionStatus.text : null}");
  });
  it("the names come from GET /entities, without the entities the gateway refused", () => {
    expect(app).toContain('gatewayRequest(gatewayApiPath("entities")).then(entityCandidates)');
    expect(app).toContain("filterMentionCandidates(await entityRoster.current.load, query, refusedEntities.current)");
  });
});

describe("a send with picked names", () => {
  it("starts the person's turn first, then asks the names, keeping the message on a refusal", () => {
    expect(app).toMatch(/await startTurn\(text, opts\?\.signal\);\s*await askMentions\(sessionId, text, mentions, \{ signal: opts\?\.signal, keepDraft: true \}\);/);
  });
  it("guidance is accepted before the names are asked; a queued message carries them to its turn", () => {
    expect(app).toMatch(/const result = await guideRun\(text\);[\s\S]{0,200}if \(result\.accepted\) await askMentions\(sessionId, text, mentions, \{ keepDraft: false \}\);/);
    expect(app).toContain("if (mode === \"queue\") return queueNextTurn(text, mentions);");
    expect(app).toMatch(/const nextRunId = await startTurn\(intent\.text\);[\s\S]{0,300}await askMentions\(intent\.sessionId, intent\.text, intent\.mentions \?\? \[\]/);
  });
  it("each mention is one POST to the session's mentions route", () => {
    expect(app).toContain("gatewayApiPath(`sessions/${encodeURIComponent(sessionId)}/mentions`)");
    expect(app).toContain("body: JSON.stringify(mentionRequestBody(text, mention))");
  });
  it("the conversation opens on the newest reply once the current one has ended, and restores the agent turn's workflow", () => {
    expect(app).toContain("if (sending || mentioning || !terminal(snapshot.status) || String(snapshot.run?.run_id || \"\") !== session.runId) return;");
    expect(app).toContain("const turn = conversationWorkflowTurn(bundle?.session?.turns, loaded);");
  });
});
