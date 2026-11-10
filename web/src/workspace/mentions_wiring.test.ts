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
    expect(chatMount).toContain("mentionStatus={mentionStatus?.sessionId === session.sessionId ? mentionStatus.text : composerAgentHint}");
  });
  it("the names come from GET /entities, without the entities the gateway refused", () => {
    expect(app).toContain('gatewayRequest(gatewayApiPath("entities")).then(entityCandidates)');
    expect(app).toContain("filterMentionCandidates(mentionRoster(agentLabel, await entityRoster.current.load), query, refusedEntities.current)");
    expect(app).toContain('const agentLabel = workflow?.name || "";');
  });
});

describe("a send with picked names", () => {
  it("starts the person's turn first, then asks the names, keeping the message on a refusal", () => {
    expect(app).toMatch(/const started = await startTurn\(text, opts\?\.signal, mentions\);\s*await askMentions\(sessionId, text, mentions, \{ repeatOf: mentionRepeatOf\(\{ started: started\.turn \}\), agentAddressed: started\.agentAddressed, signal: opts\?\.signal, keepDraft: true, resendable: true \}\);/);
  });
  it("guidance is accepted before the names are asked; a queued message carries them to its turn", () => {
    expect(app).toMatch(/const guided = session\.runId;\s*const result = await guideRun\(text\);[\s\S]{0,350}if \(result\.accepted\) await askMentions\(sessionId, text, mentions, \{ repeatOf: mentionRepeatOf\(\{ steered: guided \}\), keepDraft: false, resendable: true \}\);/);
    expect(app).toContain("if (mode === \"queue\") return queueNextTurn(text, mentions);");
    expect(app).toMatch(/const next = await startTurn\(intent\.text, undefined, intent\.mentions \?\? \[\]\);[\s\S]{0,500}await askMentions\(intent\.sessionId, intent\.text, intent\.mentions \?\? \[\], \{ repeatOf: mentionRepeatOf\(\{ started: next\.turn \}\), agentAddressed: next\.agentAddressed,/);
  });
  it("docs audit 2026-11-09: guidance (either door) names no repeat_of: the guided turn's prompt is an earlier message", () => {
    expect(app).toContain("if (!queued) await askMentions(sessionId, text, mentions, { repeatOf: mentionRepeatOf({ steered: guided }), signal: opts?.signal, keepDraft: true, resendable: true });");
    expect(app).not.toMatch(/repeatOf: guided\b/);
    expect(app.match(/mentionRepeatOf\(\{ steered: guided \}\)/g)?.length).toBe(2);
  });
  it("the names ride the turn's start (AbstractGateway c26d38a2): the agent is told the entities answer for themselves", () => {
    expect(app).toContain("async function startTurn(text: string, signal?: AbortSignal, mentions: readonly ChatMentionSent[] = []): Promise<StartedTurn> {");
    expect(app).toMatch(/const body = startRunBody\(\{\s*selection,\s*workflow,\s*sessionId: startedSession,\s*input,\s*mentions: startRunMentions\(mentions\),\s*\}\);/);
  });
  it("each mention is one POST to the session's mentions route", () => {
    expect(app).toContain("gatewayApiPath(`sessions/${encodeURIComponent(sessionId)}/mentions`)");
    expect(app).toContain("body: JSON.stringify(mentionRequestBody(text, mention, options.repeatOf, message))");
  });
  it("one answer per mention (gateway f0f23bb0): a new message keys its mention_ids from its turn; a resend names the held key", () => {
    expect(app).toContain("const message = options.message || mentionMessageKey(options.repeatOf);");
    // The resend's held key goes back, never a new one (checked just above with the resend call).
    expect(app).toMatch(/message: held\.message, signal, keepDraft: true, resendable: true/);
  });
  it("live replies are labelled from their stream frames until the saved message replaces them", () => {
    expect(app).toContain("liveAuthors.current.set(runId, author);");
    expect(app).toContain("const messages = mergeStreamNotes(withLiveAuthors(foldRepeatedTurns(baseMessages, repeatTurns.current), liveAuthors.current), [");
    expect(app).toContain("repeats: repeatTurns.current,");
  });
  it("review 2026-11-09 B1: a stop names the unsent chips and remembers the message; a resend of it asks only those, before any turn", () => {
    expect(app).toContain("pendingResend.current = { sessionId, identity: askedBy, epoch, message, resend: { text, repeatOf: options.repeatOf || null, unsent: outcome.unsent } };");
    expect(app).toContain("throw mentionsStoppedError(stop.sentence, outcome.unsent);");
    expect(app).toContain("await askMentions(held.sessionId, held.resend.text, asked, { repeatOf: held.resend.repeatOf ?? \"\", message: held.message, signal, keepDraft: true, resendable: true });");
    // The resend check runs FIRST in both doors: before startTurn / guideRun / the queue.
    expect(app).toMatch(/setMentionStatus\(null\);\s*if \(await resendMentions\(text, mentions, opts\?\.signal\)\) return;\s*if \(!active\) \{\s*const started = await startTurn/);
    expect(app).toMatch(/setMentionStatus\(null\);\s*if \(await resendMentions\(text, mentions\)\) return \{ accepted: true \};\s*if \(mode === "queue"\)/);
  });
  it("the conversation opens on the newest reply once the current one has ended, and restores the agent turn's workflow", () => {
    expect(app).toContain("if (sending || mentioning || !(terminal(snapshot.status) || unaddressedTurn) || String(snapshot.run?.run_id || \"\") !== session.runId) return;");
    expect(app).toContain("const turn = conversationWorkflowTurn(bundle?.session?.turns, loaded);");
  });
});

describe("mentions routing (operator 2026-11-10): the agent stays out of a message to entities only", () => {
  it("the start's answer decides: the recorded turn (`turn_id`) is what the mentions repeat; no agent run is bound", () => {
    expect(app).toContain("const started = startedTurn(result);");
    expect(app).toMatch(/if \(!started\.agentAddressed\) \{\s*\/\/[\s\S]{0,400}if \(started\.runId\) unaddressedTurns\.current\.add\(started\.runId\);[\s\S]{0,900}return started;\s*\}\s*attachedRun = started\.runId;/);
    // A Stop pressed while it started has no run to cancel.
    expect(app).toContain('const runId = answer?.agent === "not_addressed" ? "" : String(answer?.run_id || "");');
  });
  it("no spinner and no working dots on such a turn, live (the start) or reloaded (the gateway's marker)", () => {
    expect(app).toMatch(/const unaddressedTurn =\s*Boolean\(session\.runId\) &&\s*\(unaddressedTurns\.current\.has\(session\.runId\) \|\|\s*\(String\(snapshot\.run\?\.run_id \|\| ""\) === session\.runId && agentNotAddressed\(snapshot\.run\)\)\);\s*const active =\s*Boolean\(session\.runId\) &&\s*!unaddressedTurn &&/);
    expect(app).toContain("unaddressed: unaddressedTurns.current,");
  });
  it("the line above the composer reads the composer's chips, only for a message that starts a turn", () => {
    expect(app).toContain("const chipKinds = useComposerChipKinds(chatRoot);");
    expect(app).toContain("const composerAgentHint = !active && mentionsOff !== identity ? agentHint(chipKinds) : null;");
    expect(app).toMatch(/<main\s+className="code-conversation"\s+id="code-conversation"\s+ref=\{setChatRoot\}/);
  });
  it("only entities are asked through the mention door; no reply is said to be running without an agent run", () => {
    expect(app).toMatch(/const mentions = entityMentions\(picked\);\s*if \(!mentions\.length\) return;/);
    expect(app).toContain("replyStillRunning: Boolean(last?.runId) && options.agentAddressed !== false");
  });
  it("a person's turn restores no workflow of its own", () => {
    expect(app).toContain("if (!isSideTurn(data)) return data;");
  });
});
