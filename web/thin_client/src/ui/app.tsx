import React, { useEffect, useMemo, useRef, useState } from "react";

import { GatewayClient } from "../lib/gateway_client";
import { random_id } from "../lib/ids";
import { McpWorkerClient } from "../lib/mcp_worker_client";
import { extract_emit_event, extract_tool_calls_from_wait, extract_wait_from_record } from "../lib/runtime_extractors";
import { LedgerStreamEvent, StepRecord, ToolCall, ToolResult, WaitState } from "../lib/types";

type Settings = {
  gateway_url: string;
  auth_token: string;
  worker_url: string;
  worker_token: string;
};

type UiLogItem = {
  ts: string;
  kind: "step" | "event" | "message" | "error" | "info";
  title: string;
  body?: string;
};

function now_iso(): string {
  return new Date().toISOString();
}

function safe_json(v: any): string {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

function load_settings(): Settings {
  try {
    const raw = localStorage.getItem("abstractcode_thin_client_settings");
    if (!raw) throw new Error("missing");
    const parsed = JSON.parse(raw);
    return {
      gateway_url: String(parsed?.gateway_url || ""),
      auth_token: String(parsed?.auth_token || ""),
      worker_url: String(parsed?.worker_url || ""),
      worker_token: String(parsed?.worker_token || ""),
    };
  } catch {
    return { gateway_url: "", auth_token: "", worker_url: "", worker_token: "" };
  }
}

function save_settings(s: Settings): void {
  localStorage.setItem("abstractcode_thin_client_settings", JSON.stringify(s));
}

function format_step_summary(rec: StepRecord): string {
  const node = String(rec?.node_id || "");
  const st = String(rec?.status || "");
  const eff = String(rec?.effect?.type || "");
  return `${node || "(node?)"} • ${st || "(status?)"} • ${eff || "(effect?)"}`;
}

function is_waiting_status(rec: StepRecord | null): boolean {
  return Boolean(rec && String(rec.status || "") === "waiting");
}

export function App(): React.ReactElement {
  const [settings, set_settings] = useState<Settings>(() => load_settings());
  const [run_id, set_run_id] = useState<string>("");
  const [flow_id, set_flow_id] = useState<string>("");

  const [connected, set_connected] = useState(false);
  const [connecting, set_connecting] = useState(false);
  const [cursor, set_cursor] = useState<number>(0);
  const [records, set_records] = useState<Array<{ cursor: number; record: StepRecord }>>([]);
  const cursor_ref = useRef<number>(0);
  const [run_state, set_run_state] = useState<any>(null);
  const [control_reason, set_control_reason] = useState<string>("");

  const [status_text, set_status_text] = useState<string>("");
  const status_timer_ref = useRef<number | null>(null);

  const [log, set_log] = useState<UiLogItem[]>([]);
  const [error_text, set_error_text] = useState<string>("");

  const abort_ref = useRef<AbortController | null>(null);

  const gateway = useMemo(() => new GatewayClient({ base_url: settings.gateway_url, auth_token: settings.auth_token }), [settings]);
  const worker = useMemo(
    () => (settings.worker_url.trim() ? new McpWorkerClient({ url: settings.worker_url.trim(), auth_token: settings.worker_token }) : null),
    [settings.worker_url, settings.worker_token]
  );

  const last_record = records.length ? records[records.length - 1].record : null;
  const wait_state: WaitState | null = useMemo(() => extract_wait_from_record(last_record), [last_record]);

  useEffect(() => {
    save_settings(settings);
  }, [settings]);

  useEffect(() => {
    return () => {
      if (abort_ref.current) abort_ref.current.abort();
      if (status_timer_ref.current) window.clearTimeout(status_timer_ref.current);
    };
  }, []);

  // Best-effort run state polling (pause/cancel are run-level changes that are not currently ledgered).
  useEffect(() => {
    const rid = run_id.trim();
    if (!connected || !rid) return;

    let stopped = false;
    const poll = async () => {
      try {
        const st = await gateway.get_run(rid);
        if (!stopped) set_run_state(st);
      } catch {
        // ignore
      }
    };

    poll();
    const timer = window.setInterval(poll, 2000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [connected, run_id, gateway]);

  function push_log(item: UiLogItem): void {
    set_log((prev) => [item, ...prev].slice(0, 200));
  }

  function set_status(text: string, duration_s: number): void {
    set_status_text(text);
    if (status_timer_ref.current) {
      window.clearTimeout(status_timer_ref.current);
      status_timer_ref.current = null;
    }
    if (duration_s > 0) {
      status_timer_ref.current = window.setTimeout(() => {
        set_status_text("");
        status_timer_ref.current = null;
      }, Math.max(1, duration_s) * 1000);
    }
  }

  function handle_step(ev: LedgerStreamEvent): void {
    cursor_ref.current = ev.cursor;
    set_cursor(ev.cursor);
    set_records((prev) => [...prev, { cursor: ev.cursor, record: ev.record }]);

    const emit = extract_emit_event(ev.record);
    if (emit && emit.name.startsWith("abstractcode.")) {
      if (emit.name === "abstractcode.status") {
        const payload = emit.payload;
        const text = typeof payload?.text === "string" ? payload.text : typeof payload === "string" ? payload : safe_json(payload);
        const duration = typeof payload?.duration === "number" ? payload.duration : -1;
        set_status(text, duration);
      } else if (emit.name === "abstractcode.message") {
        const payload = emit.payload;
        const text =
          typeof payload?.text === "string"
            ? payload.text
            : typeof payload?.message === "string"
              ? payload.message
              : typeof payload === "string"
                ? payload
                : safe_json(payload);
        push_log({ ts: now_iso(), kind: "message", title: "Message", body: text });
      } else if (emit.name === "abstractcode.tool_execution") {
        push_log({ ts: now_iso(), kind: "event", title: "Tool execution", body: safe_json(emit.payload) });
      } else if (emit.name === "abstractcode.tool_result") {
        push_log({ ts: now_iso(), kind: "event", title: "Tool result", body: safe_json(emit.payload) });
      } else {
        push_log({ ts: now_iso(), kind: "event", title: emit.name, body: safe_json(emit.payload) });
      }
    } else {
      // Default trace log
      push_log({ ts: now_iso(), kind: "step", title: format_step_summary(ev.record), body: safe_json(ev.record) });
    }
  }

  async function replay_ledger(run_id_value: string, opts: { after: number }): Promise<number> {
    let after = opts.after;
    while (true) {
      const page = await gateway.get_ledger(run_id_value, { after, limit: 200 });
      const items = Array.isArray(page.items) ? page.items : [];
      if (!items.length) {
        set_cursor(after);
        cursor_ref.current = after;
        return after;
      }
      for (const item of items) {
        const c = typeof item?.cursor === "number" ? item.cursor : null;
        const r = item?.record;
        if (typeof c === "number" && r) {
          handle_step({ cursor: c, record: r });
          after = c;
        }
      }
      after = typeof page.next_after === "number" ? page.next_after : after;
    }
  }

  async function connect_to_run(run_id_value: string): Promise<void> {
    set_error_text("");
    set_connecting(true);
    set_connected(false);
    set_records([]);
    set_cursor(0);
    cursor_ref.current = 0;

    if (abort_ref.current) abort_ref.current.abort();
    const abort = new AbortController();
    abort_ref.current = abort;

    try {
      let after = await replay_ledger(run_id_value, { after: 0 });
      set_connected(true);
      push_log({ ts: now_iso(), kind: "info", title: `Attached to run ${run_id_value}` });

      let backoff_ms = 250;
      while (!abort.signal.aborted) {
        // Best-effort resync before streaming (replay-first).
        after = await replay_ledger(run_id_value, { after: cursor_ref.current });
        try {
          await gateway.stream_ledger(run_id_value, {
            after,
            on_step: handle_step,
            signal: abort.signal,
          });
        } catch (e: any) {
          if (abort.signal.aborted) break;
          const msg = String(e?.message || e || "stream error");
          push_log({ ts: now_iso(), kind: "error", title: "Ledger stream error (will retry)", body: msg });
        }

        if (abort.signal.aborted) break;
        await new Promise((r) => setTimeout(r, backoff_ms));
        backoff_ms = Math.min(5000, Math.floor(backoff_ms * 1.6));
      }
    } catch (e: any) {
      const msg = String(e?.message || e || "unknown error");
      set_error_text(msg);
      push_log({ ts: now_iso(), kind: "error", title: "Connection error", body: msg });
      set_connected(false);
    } finally {
      set_connecting(false);
    }
  }

  async function on_start_run(): Promise<void> {
    const fid = flow_id.trim();
    if (!fid) {
      set_error_text("Missing flow_id");
      return;
    }
    set_error_text("");
    set_connecting(true);
    try {
      const rid = await gateway.start_run(fid, {});
      set_run_id(rid);
      await connect_to_run(rid);
    } catch (e: any) {
      set_error_text(String(e?.message || e || "start failed"));
    } finally {
      set_connecting(false);
    }
  }

  async function on_attach_run(): Promise<void> {
    const rid = run_id.trim();
    if (!rid) {
      set_error_text("Missing run_id");
      return;
    }
    await connect_to_run(rid);
  }

  async function submit_run_control(type: "pause" | "resume" | "cancel"): Promise<void> {
    const rid = run_id.trim();
    if (!rid) {
      set_error_text("Missing run_id");
      return;
    }
    set_error_text("");
    try {
      const payload: any = {};
      const reason = control_reason.trim();
      if (reason) payload.reason = reason;
      await gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type,
        payload,
        client_id: "web_pwa",
      });
      push_log({ ts: now_iso(), kind: "info", title: `${type} submitted`, body: reason ? `reason: ${reason}` : "" });
      // Refresh run state quickly.
      try {
        const st = await gateway.get_run(rid);
        set_run_state(st);
      } catch {
        // ignore
      }
    } catch (e: any) {
      set_error_text(String(e?.message || e || `${type} failed`));
    }
  }

  async function resume_wait(payload_obj: any): Promise<void> {
    const rid = run_id.trim();
    const wk = String(wait_state?.wait_key || "").trim();
    if (!rid || !wk) {
      set_error_text("No active wait to resume");
      return;
    }
    set_error_text("");
    try {
      await gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type: "resume",
        payload: { wait_key: wk, payload: payload_obj || {} },
        client_id: "web_pwa",
      });
      push_log({ ts: now_iso(), kind: "info", title: "Resume submitted", body: safe_json({ wait_key: wk }) });
    } catch (e: any) {
      set_error_text(String(e?.message || e || "resume failed"));
    }
  }

  async function execute_tools_via_worker(tool_calls: ToolCall[]): Promise<void> {
    if (!worker) {
      set_error_text("No worker configured");
      return;
    }
    set_error_text("");

    const results: ToolResult[] = [];
    for (const tc of tool_calls) {
      // Sequential to keep UX predictable (and avoid flooding).
      // Future: bounded concurrency + cancellation.
      const res = await worker.call_tool(tc);
      results.push(res);
    }

    await resume_wait({ mode: "executed", results });
  }

  const tool_calls_for_wait = useMemo(() => extract_tool_calls_from_wait(wait_state), [wait_state]);
  const show_wait_modal = is_waiting_status(last_record) && wait_state && wait_state.wait_key;

  return (
    <div className="container">
      <div className="title">
        <h1>AbstractCode Thin Client (Web/PWA)</h1>
        <div className="badge mono">
          {connected ? "connected" : connecting ? "connecting…" : "disconnected"} • cursor {cursor}
        </div>
      </div>

      <div className="row">
        <div className="col">
          <div className="card">
            <div className="field">
              <label>Gateway URL (blank = same origin / dev proxy)</label>
              <input
                className="mono"
                value={settings.gateway_url}
                onChange={(e) => set_settings((s) => ({ ...s, gateway_url: e.target.value }))}
                placeholder="https://your-gateway-host"
              />
            </div>
            <div className="field">
              <label>Gateway token (Authorization: Bearer …)</label>
              <input
                className="mono"
                value={settings.auth_token}
                onChange={(e) => set_settings((s) => ({ ...s, auth_token: e.target.value }))}
                placeholder="(optional for localhost dev)"
              />
            </div>
            <div className="row">
              <div className="col">
                <div className="field">
                  <label>Flow ID (optional: start run)</label>
                  <input className="mono" value={flow_id} onChange={(e) => set_flow_id(e.target.value)} placeholder="a803f4bd" />
                </div>
              </div>
              <div className="col">
                <div className="field">
                  <label>Run ID (attach)</label>
                  <input className="mono" value={run_id} onChange={(e) => set_run_id(e.target.value)} placeholder="run uuid" />
                </div>
              </div>
            </div>

            <div className="actions">
              <button className="btn primary" onClick={on_start_run} disabled={connecting}>
                Start run
              </button>
              <button className="btn" onClick={on_attach_run} disabled={connecting}>
                Attach
              </button>
              <button className="btn" onClick={() => submit_run_control("pause")} disabled={!run_id.trim() || connecting}>
                Pause
              </button>
              <button className="btn" onClick={() => submit_run_control("resume")} disabled={!run_id.trim() || connecting}>
                Resume
              </button>
              <button className="btn danger" onClick={() => submit_run_control("cancel")} disabled={!run_id.trim() || connecting}>
                Cancel
              </button>
              <button
                className="btn danger"
                onClick={() => {
                  if (abort_ref.current) abort_ref.current.abort();
                  set_connected(false);
                  set_connecting(false);
                }}
              >
                Disconnect
              </button>
            </div>

            <div className="field">
              <label>Run control reason (optional)</label>
              <input className="mono" value={control_reason} onChange={(e) => set_control_reason(e.target.value)} placeholder="reason…" />
            </div>

            {run_state ? (
              <div className="log_item">
                <div className="meta">
                  <span className="mono">run state</span>
                  <span className="mono">{String(run_state?.status || "")}</span>
                </div>
                <div className="body mono">
                  {safe_json({
                    status: run_state?.status,
                    paused: run_state?.paused,
                    current_node: run_state?.current_node,
                    waiting: run_state?.waiting ? { reason: run_state.waiting.reason, prompt: run_state.waiting.prompt } : null,
                    error: run_state?.error,
                  })}
                </div>
              </div>
            ) : null}

            <div className="field">
              <label>Tool worker (advanced / potentially dangerous) — MCP HTTP endpoint</label>
              <input
                className="mono"
                value={settings.worker_url}
                onChange={(e) => set_settings((s) => ({ ...s, worker_url: e.target.value }))}
                placeholder="https://your-mcp-worker-endpoint"
              />
            </div>
            <div className="field">
              <label>Tool worker token (Authorization: Bearer …)</label>
              <input
                className="mono"
                value={settings.worker_token}
                onChange={(e) => set_settings((s) => ({ ...s, worker_token: e.target.value }))}
                placeholder="(optional)"
              />
            </div>

            {error_text ? (
              <div className="log_item" style={{ borderColor: "rgba(239, 68, 68, 0.35)" }}>
                <div className="meta">
                  <span className="mono">error</span>
                  <span className="mono">{now_iso()}</span>
                </div>
                <div className="body mono">{error_text}</div>
              </div>
            ) : null}
          </div>
        </div>

        <div className="col">
          <div className="card">
            <div className="meta">
              <span className="mono">ledger log (newest first)</span>
            </div>
            <div className="log">
              {log.map((item, idx) => (
                <div key={idx} className="log_item">
                  <div className="meta">
                    <span className="mono">
                      {item.kind} • {item.title}
                    </span>
                    <span className="mono">{item.ts}</span>
                  </div>
                  {item.body ? <div className="body mono">{item.body}</div> : null}
                </div>
              ))}
            </div>
          </div>

          <div className="status_bar">
            <strong>Status</strong>: {status_text ? <span className="mono">{status_text}</span> : <span className="mono">(none)</span>}
          </div>
        </div>
      </div>

      {show_wait_modal ? (
        <div className="overlay">
          <div className="modal">
            <h2 className="mono">Run is waiting ({String(wait_state?.reason || "unknown")})</h2>
            <p className="mono">wait_key: {String(wait_state?.wait_key || "")}</p>

            {tool_calls_for_wait.length ? (
              <>
                <div className="field">
                  <label>Tool calls (from wait.details.tool_calls)</label>
                  <textarea className="mono" readOnly value={safe_json(tool_calls_for_wait)} />
                </div>
                <div className="actions">
                  <button className="btn primary" disabled={!worker} onClick={() => execute_tools_via_worker(tool_calls_for_wait)}>
                    Execute via tool worker + resume
                  </button>
                  <button className="btn" onClick={() => resume_wait({ approved: true })}>
                    Resume (manual / advanced)
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className="field">
                  <label>Prompt</label>
                  <textarea className="mono" readOnly value={String(wait_state?.prompt || "")} />
                </div>

                <AskForm wait={wait_state} on_submit={(val) => resume_wait({ response: val })} />
              </>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function AskForm(props: { wait: WaitState; on_submit: (value: string) => void }): React.ReactElement {
  const [value, set_value] = useState("");
  const choices = Array.isArray(props.wait.choices) ? props.wait.choices : [];
  const allow_free_text = props.wait.allow_free_text !== false;

  return (
    <>
      {choices.length ? (
        <div className="field">
          <label>Choices</label>
          <select className="mono" value={value} onChange={(e) => set_value(e.target.value)}>
            <option value="">(select)</option>
            {choices.map((c, idx) => (
              <option key={idx} value={String(c)}>
                {String(c)}
              </option>
            ))}
          </select>
        </div>
      ) : null}

      {allow_free_text ? (
        <div className="field">
          <label>Response</label>
          <input className="mono" value={value} onChange={(e) => set_value(e.target.value)} placeholder="Type response…" />
        </div>
      ) : null}

      <div className="actions">
        <button className="btn primary" disabled={!value.trim()} onClick={() => props.on_submit(value.trim())}>
          Submit response
        </button>
      </div>
    </>
  );
}


