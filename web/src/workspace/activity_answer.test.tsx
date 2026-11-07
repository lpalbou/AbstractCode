// The run's final answer in the Activity panel (web 0.15.6): the whole text, rendered with the
// chat's Markdown, folded to a few lines with a Show more / Show less toggle. It used to be
// flattened to one line and cut at 110 characters ("It's a solid interface. A few thoughts:
// **What works well:** - **The Activity panel** is the standout feature…").
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { activity_rows, activity_rows_from_view, type ActivityRow } from "../lib/activity_rows";
import { ActivityAnswerView, ActivityGroups } from "./workspace_panels";

const ANSWER = [
  "It's a solid interface. A few thoughts:",
  "",
  "**What works well:**",
  "- **The Activity panel** is the standout feature: every step is visible.",
  "- The rail keeps files and settings one click away.",
  "",
  "**What could improve:** the composer could say which model answers.",
].join("\n");

function ledgerRows(answer: string): ActivityRow[] {
  return activity_rows(
    [{ runId: "root1", cursor: 1, record: { run_id: "root1", step_id: "s1", status: "completed", result: { completed: true, output: { answer } } } }],
    "root1",
  );
}

function viewRows(answer: string): ActivityRow[] {
  return activity_rows_from_view({
    root_run_id: "root1",
    activities: [],
    runs: [{ run_id: "root1", status: "completed", last_seq: 4 }],
    messages: [{ id: "root1:final", run_id: "root1", text: answer, order: ["2026-11-07T10:00:00Z", "root1", 4] }],
  });
}

describe("run finished: the whole answer, as Markdown", () => {
  it("the ledger row carries the answer uncut and marks it as Markdown", () => {
    const finished = ledgerRows(ANSWER).find((row) => row.title === "run finished");
    expect(ANSWER.length).toBeGreaterThan(110);
    expect(finished?.detail).toBe(ANSWER);
    expect(finished?.detailMarkdown).toBe(true);
  });

  it("the view row (replay view) carries the same uncut answer", () => {
    const finished = viewRows(ANSWER).find((row) => row.title === "run finished");
    expect(finished?.detail).toBe(ANSWER);
    expect(finished?.detailMarkdown).toBe(true);
  });

  it("a stop reason stays a short plain line (not Markdown)", () => {
    const rows = activity_rows(
      [{ runId: "r", cursor: 1, record: { run_id: "r", status: "completed", result: { completed: true, output: { answer: ANSWER, stop_reason: { code: "max_iterations" }, iterations: 3 } } } }],
      "r",
    );
    expect(rows[0].detail).toBe("max iterations · 3 iterations");
    expect(rows[0].detailMarkdown).toBeUndefined();
  });

  it("renders bold and list Markdown as elements inside the Activity row, outside its summary", () => {
    const html = renderToStaticMarkup(<ActivityGroups rows={ledgerRows(ANSWER)} />);
    expect(html).toContain("<strong>What works well:</strong>");
    expect(html).toContain("<strong>The Activity panel</strong>");
    expect(html).toMatch(/<ul class="pc-md_ul">\s*<li[^>]*>/);
    expect(html).toContain("every step is visible.");
    expect(html).not.toContain("**What works well:**");
    const summary = html.match(/<details class="code-activity-disclosure"><summary>[\s\S]*?<\/summary>/)?.[0] || "";
    expect(summary).toContain("run finished");
    expect(summary).not.toContain("What works well");
  });

  it("no raw HTML from the model reaches the page, and unsafe links do not become links", () => {
    const hostile = "Done.\n\n<img src=x onerror=alert(1)> <script>alert(2)</script>\n\n[click](javascript:alert(3)) and [docs](https://example.com/docs)";
    const html = renderToStaticMarkup(<ActivityGroups rows={ledgerRows(hostile)} />);
    expect(html).not.toMatch(/<img|<script/);
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(html).not.toContain('href="javascript:');
    expect(html).toContain('href="https://example.com/docs"');
    expect(html).toContain('rel="noopener noreferrer"');
  });

  it("other rows keep their short plain detail line", () => {
    const html = renderToStaticMarkup(
      <ActivityGroups rows={[{ ...ledgerRows("x")[0], key: "m", kind: "message", title: "message", detail: "**plain**", detailMarkdown: undefined }]} />,
    );
    expect(html).toContain('<span class="code-activity-sub">**plain**</span>');
    expect(html).not.toContain("code-activity-answer");
  });
});

describe("the answer's fold: Show more / Show less", () => {
  const view = (expanded: boolean, overflows = true, onToggle = () => {}) =>
    renderToStaticMarkup(<ActivityAnswerView id="a1" text={ANSWER} expanded={expanded} overflows={overflows} onToggle={onToggle} />);

  it("folded: the body is clamped and faded, the toggle says Show more and is not expanded", () => {
    const html = view(false);
    expect(html).toContain('class="code-activity-answer-body is-folded"');
    expect(html).toMatch(/<button type="button" class="code-text-button code-activity-answer-toggle" aria-expanded="false" aria-controls="a1">Show more<\/button>/);
  });

  it("expanded: the body is unclamped, the toggle says Show less and is expanded", () => {
    const html = view(true);
    expect(html).toContain('class="code-activity-answer-body is-expanded"');
    expect(html).toMatch(/aria-expanded="true" aria-controls="a1">Show less<\/button>/);
  });

  it("a short answer that fits needs no toggle", () => {
    expect(view(false, false)).not.toContain("<button");
  });

  it("the toggle is a real button wired to the fold state", () => {
    const onToggle = vi.fn();
    const element = ActivityAnswerView({ id: "a1", text: ANSWER, expanded: false, overflows: true, onToggle });
    const children = React.Children.toArray((element as React.ReactElement).props.children) as React.ReactElement[];
    const button = children.find((child) => child.type === "button")!;
    expect(button.props.type).toBe("button");
    button.props.onClick();
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
