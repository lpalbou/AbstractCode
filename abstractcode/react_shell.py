from __future__ import annotations

import json
import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple


def _supports_color() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    return bool(getattr(sys.stdout, "isatty", lambda: False)())


class _C:
    RESET = "\033[0m"
    DIM = "\033[2m"
    BOLD = "\033[1m"
    CYAN = "\033[36m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    MAGENTA = "\033[35m"
    RED = "\033[31m"


def _style(text: str, *codes: str, enabled: bool) -> str:
    if not enabled or not codes:
        return text
    return "".join(codes) + text + _C.RESET


@dataclass
class _ToolSpec:
    name: str
    description: str
    parameters: Dict[str, Any]


class ReactShell:
    def __init__(
        self,
        *,
        provider: str,
        model: str,
        state_file: Optional[str],
        auto_approve: bool,
        max_iterations: int,
        color: bool,
    ):
        self._provider = provider
        self._model = model
        self._state_file = state_file or None
        self._auto_approve = auto_approve
        self._max_iterations = int(max_iterations)
        if self._max_iterations < 1:
            raise ValueError("max_iterations must be >= 1")
        self._color = bool(color and _supports_color())

        # Lazy imports so `abstractcode --help` works even if deps aren't installed.
        try:
            from abstractagent.agents.react import ReactAgent
            from abstractagent.tools import ALL_TOOLS
            from abstractcore.tools import ToolDefinition
            from abstractruntime import InMemoryLedgerStore, InMemoryRunStore, JsonFileRunStore, JsonlLedgerStore
            from abstractruntime.core.models import RunStatus, WaitReason
            from abstractruntime.integrations.abstractcore import (
                MappingToolExecutor,
                PassthroughToolExecutor,
                create_local_runtime,
            )
        except Exception as e:  # pragma: no cover
            raise SystemExit(
                "AbstractCode requires AbstractAgent/AbstractRuntime/AbstractCore to be importable.\n"
                "In this monorepo, run with:\n"
                "  PYTHONPATH=abstractcode:abstractagent/src:abstractruntime/src:abstractcore python -m abstractcode.cli\n"
                f"\nImport error: {e}"
            )

        self._RunStatus = RunStatus
        self._WaitReason = WaitReason

        self._tools: List[Callable[..., Any]] = list(ALL_TOOLS)
        self._tool_specs: Dict[str, _ToolSpec] = {}
        for t in self._tools:
            tool_def = getattr(t, "_tool_definition", None) or ToolDefinition.from_function(t)
            self._tool_specs[tool_def.name] = _ToolSpec(
                name=tool_def.name,
                description=tool_def.description,
                parameters=dict(tool_def.parameters or {}),
            )

        store_dir: Optional[Path] = None
        # Stores: file-backed only when state_file is provided.
        if self._state_file:
            base = Path(self._state_file).expanduser().resolve()
            base.parent.mkdir(parents=True, exist_ok=True)
            store_dir = base.with_name(base.stem + ".d")
            run_store = JsonFileRunStore(store_dir)
            ledger_store = JsonlLedgerStore(store_dir)
        else:
            run_store = InMemoryRunStore()
            ledger_store = InMemoryLedgerStore()

        # Tool execution: passthrough by default so we can gate by approval in the CLI.
        tool_executor = PassthroughToolExecutor(mode="approval_required")
        self._tool_runner = MappingToolExecutor.from_tools(self._tools)
        self._runtime = create_local_runtime(
            provider=self._provider,
            model=self._model,
            run_store=run_store,
            ledger_store=ledger_store,
            tool_executor=tool_executor,
        )

        self._agent = ReactAgent(
            runtime=self._runtime,
            tools=self._tools,
            on_step=self._on_step,
            max_iterations=self._max_iterations,
        )

        self._store_dir = store_dir
        self._approve_all_for_run = False

    # ---------------------------------------------------------------------
    # UI helpers
    # ---------------------------------------------------------------------

    def _print(self, text: str = "") -> None:
        print(text)

    def _banner(self) -> None:
        self._print(_style("AbstractCode (MVP)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(f"Provider: {self._provider}   Model: {self._model}")
        if self._state_file:
            store = str(self._store_dir) + "/" if self._store_dir else "(unknown)"
            self._print(f"State:    {self._state_file} (store: {store})")
        else:
            self._print("State:    (in-memory; cannot resume after quitting)")
        mode = "auto-approve" if self._auto_approve else "approval-gated"
        self._print(f"Tools:    {len(self._tools)} ({mode})")
        self._print(_style("Type '/help' for commands.", _C.DIM, enabled=self._color))

    def _on_step(self, step: str, data: Dict[str, Any]) -> None:
        if step == "init":
            task = (data.get("task") or "")[:80]
            self._print(_style("\nStarting:", _C.CYAN, _C.BOLD, enabled=self._color) + f" {task}")
        elif step == "reason":
            it = data.get("iteration", "?")
            self._print(_style(f"Thinking (step {it})...", _C.YELLOW, enabled=self._color))
        elif step == "act":
            tool = data.get("tool", "unknown")
            args = data.get("args") or {}
            args_str = json.dumps(args, ensure_ascii=False)
            if len(args_str) > 100:
                args_str = args_str[:97] + "..."
            self._print(_style("Tool:", _C.GREEN, enabled=self._color) + f" {tool}({args_str})")
        elif step == "observe":
            res = str(data.get("result", ""))[:120]
            self._print(_style("Result:", _C.DIM, enabled=self._color) + f" {res}")
        elif step == "ask_user":
            self._print(_style("Agent question:", _C.MAGENTA, _C.BOLD, enabled=self._color))
        elif step == "done":
            self._print(_style("\nANSWER", _C.GREEN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
            self._print(str(data.get("answer", "")))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    # ---------------------------------------------------------------------
    # Commands
    # ---------------------------------------------------------------------

    def run(self) -> None:
        self._banner()
        self._show_tools()

        if self._state_file:
            self._try_load_state()

        while True:
            try:
                user_input = input(_style("\n> ", _C.CYAN, _C.BOLD, enabled=self._color)).strip()
            except (EOFError, KeyboardInterrupt):
                self._print()
                break

            if not user_input:
                continue

            cmd = user_input.strip()

            if cmd.startswith("/"):
                should_exit = self._dispatch_command(cmd[1:].strip())
                if should_exit:
                    break
                continue

            # Reserved words are commands (but require a leading slash).
            lower = cmd.lower()
            if lower in ("help", "tools", "status", "history", "resume", "quit", "exit", "q", "task"):
                self._print(_style("Commands must start with '/'.", _C.DIM, enabled=self._color))
                self._print(_style(f"Try: /{lower}", _C.DIM, enabled=self._color))
                continue

            # Otherwise treat as a task.
            self._start(cmd)

    def _dispatch_command(self, raw: str) -> bool:
        if not raw:
            return False

        parts = raw.split(None, 1)
        command = parts[0].lower()
        arg = parts[1] if len(parts) > 1 else ""

        if command in ("quit", "exit", "q"):
            return True
        if command in ("help", "h", "?"):
            self._show_help()
            return False
        if command == "tools":
            self._show_tools()
            return False
        if command == "status":
            self._show_status()
            return False
        if command in ("auto-accept", "auto_accept"):
            self._set_auto_accept(arg)
            return False
        if command == "resume":
            self._resume()
            return False
        if command == "history":
            limit = 12
            if arg:
                try:
                    limit = int(arg)
                except ValueError:
                    self._print(_style("Usage: /history [N]", _C.DIM, enabled=self._color))
                    return False
            self._show_history(limit=limit)
            return False
        if command == "task":
            task = arg.strip()
            if not task:
                self._print(_style("Usage: /task <your task>", _C.DIM, enabled=self._color))
                return False
            self._start(task)
            return False

        self._print(_style(f"Unknown command: /{command}", _C.YELLOW, enabled=self._color))
        self._print(_style("Type /help for commands.", _C.DIM, enabled=self._color))
        return False

    def _set_auto_accept(self, raw: str) -> None:
        value = raw.strip().lower()
        if not value:
            self._auto_approve = not self._auto_approve
        elif value in ("on", "true", "1", "yes", "y"):
            self._auto_approve = True
        elif value in ("off", "false", "0", "no", "n"):
            self._auto_approve = False
        else:
            self._print(_style("Usage: /auto-accept [on|off]", _C.DIM, enabled=self._color))
            return

        status = "ON (no approval prompts)" if self._auto_approve else "OFF (approval-gated)"
        self._print(_style(f"Auto-accept is now {status}.", _C.DIM, enabled=self._color))

    def _show_help(self) -> None:
        self._print(
            "\nCommands:\n"
            "  /help           Show this message\n"
            "  /tools          List available tools\n"
            "  /status         Show current run status\n"
            "  /auto-accept    Toggle auto-accept for tools (or: /auto-accept on|off)\n"
            "  /history [N]    Show recent conversation history\n"
            "  /resume         Resume the saved/attached run\n"
            "  /quit           Exit\n"
            "\nTasks:\n"
            "  /task <text>    Start a new task\n"
            "  <text>          Start a new task (any line not starting with '/')\n"
        )

    def _show_tools(self) -> None:
        self._print(_style("\nAvailable tools", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        for name, spec in sorted(self._tool_specs.items()):
            params = ", ".join(sorted((spec.parameters or {}).keys()))
            self._print(f"- {name}({params})")
            self._print(_style(f"  {spec.description}", _C.DIM, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    def _show_status(self) -> None:
        state = self._agent.get_state()
        if state is None:
            self._print("No active run.")
            return

        self._print(_style("\nRun status", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))
        self._print(f"Run ID:    {state.run_id}")
        self._print(f"Workflow:  {state.workflow_id}")
        self._print(f"Status:    {state.status.value}")
        self._print(f"Node:      {state.current_node}")
        if state.waiting:
            self._print(f"Waiting:   {state.waiting.reason.value}")
            if state.waiting.prompt:
                self._print(f"Prompt:    {state.waiting.prompt}")
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))

    def _messages_from_state(self, state: Any) -> List[Dict[str, Any]]:
        context = state.vars.get("context") if hasattr(state, "vars") else None
        if isinstance(context, dict) and isinstance(context.get("messages"), list):
            return list(context["messages"])
        if hasattr(state, "vars") and isinstance(state.vars.get("messages"), list):
            return list(state.vars["messages"])
        if getattr(state, "output", None) and isinstance(state.output.get("messages"), list):
            return list(state.output["messages"])
        return []

    def _show_history(self, *, limit: int = 12) -> None:
        state = self._agent.get_state()
        if state is None:
            messages = list(self._agent.session_messages or [])
        else:
            messages = self._messages_from_state(state)
        if not messages:
            self._print("No history yet.")
            return

        self._print(_style("\nHistory", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        for m in messages[-limit:]:
            role = m.get("role", "unknown")
            content = (m.get("content") or "").strip()
            if len(content) > 240:
                content = content[:237] + "..."
            self._print(f"{role}: {content}")
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    # ---------------------------------------------------------------------
    # Execution
    # ---------------------------------------------------------------------

    def _start(self, task: str) -> None:
        self._approve_all_for_run = False
        run_id = self._agent.start(task)
        if self._state_file:
            self._agent.save_state(self._state_file)
        self._run_loop(run_id)

    def _resume(self) -> None:
        if self._agent.run_id is None and self._state_file:
            self._try_load_state()

        run_id = self._agent.run_id
        if run_id is None:
            self._print("No run to resume.")
            return

        self._run_loop(run_id)

    def _try_load_state(self) -> None:
        try:
            state = self._agent.load_state(self._state_file)  # type: ignore[arg-type]
        except Exception as e:
            self._print(_style("State load failed:", _C.YELLOW, enabled=self._color) + f" {e}")
            return
        if state is not None:
            messages: Optional[List[Dict[str, Any]]] = None
            loaded = self._messages_from_state(state)
            if loaded:
                messages = loaded

            if messages is not None:
                self._agent.session_messages = messages

            if state.status == self._RunStatus.WAITING:
                msg = "Loaded saved run. Type '/resume' to continue."
            else:
                msg = "Loaded history from last run."
            self._print(_style(msg, _C.DIM, enabled=self._color))

    def _run_loop(self, run_id: str) -> None:
        while True:
            try:
                state = self._agent.step()
            except KeyboardInterrupt:
                state = self._agent.get_state()
                if state is not None:
                    loaded = self._messages_from_state(state)
                    if loaded:
                        self._agent.session_messages = loaded
                self._print(_style("\nInterrupted. Run state preserved.", _C.YELLOW, enabled=self._color))
                return

            if state.status == self._RunStatus.COMPLETED:
                if state.output and isinstance(state.output.get("messages"), list):
                    self._agent.session_messages = list(state.output["messages"])
                return

            if state.status == self._RunStatus.FAILED:
                self._print(_style("\nRun failed:", _C.RED, enabled=self._color) + f" {state.error}")
                loaded = self._messages_from_state(state)
                if loaded:
                    self._agent.session_messages = loaded
                return

            if state.status != self._RunStatus.WAITING or not state.waiting:
                # Either still RUNNING (max_steps exceeded) or some other non-blocking state.
                continue

            wait = state.waiting
            if wait.reason == self._WaitReason.USER:
                response = self._prompt_user(wait.prompt or "Please respond:", wait.choices)
                state = self._agent.resume(response)
                continue

            # Tool approval waits are modeled as EVENT waits with details.tool_calls.
            details = wait.details or {}
            tool_calls = details.get("tool_calls")
            if isinstance(tool_calls, list):
                payload = self._approve_and_execute(tool_calls)
                if payload is None:
                    self._print(_style("\nLeft run waiting (not resumed).", _C.DIM, enabled=self._color))
                    return

                state = self._runtime.resume(
                    workflow=self._agent.workflow,
                    run_id=run_id,
                    wait_key=wait.wait_key,
                    payload=payload,
                )
                continue

            self._print(
                _style("\nWaiting:", _C.YELLOW, enabled=self._color)
                + f" {wait.reason.value} ({wait.wait_key})"
            )
            return

    def _prompt_user(self, prompt: str, choices: Optional[Sequence[str]]) -> str:
        if choices:
            self._print(_style(prompt, _C.MAGENTA, _C.BOLD, enabled=self._color))
            for i, c in enumerate(choices):
                self._print(f"  [{i+1}] {c}")
            while True:
                raw = input("Choice (number or text): ").strip()
                if not raw:
                    continue
                if raw.isdigit():
                    idx = int(raw) - 1
                    if 0 <= idx < len(choices):
                        return str(choices[idx])
                return raw
        return input(prompt + " ").strip()

    def _approve_and_execute(self, tool_calls: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
        if self._auto_approve:
            return self._tool_runner.execute(tool_calls=tool_calls)

        self._print(_style("\nTool approval required", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

        approve_all = bool(self._approve_all_for_run)
        results: List[Dict[str, Any]] = []

        for tc in tool_calls:
            name = str(tc.get("name", "") or "")
            args = dict(tc.get("arguments") or {})
            call_id = str(tc.get("call_id") or "")

            spec = self._tool_specs.get(name)
            descr = spec.description if spec else ""

            self._print(_style(f"\n{name}", _C.GREEN, _C.BOLD, enabled=self._color))
            if descr:
                self._print(_style(descr, _C.DIM, enabled=self._color))
            self._print(
                _style("args:", _C.DIM, enabled=self._color)
                + " "
                + json.dumps(_truncate_json(args), indent=2, ensure_ascii=False)
            )

            if not approve_all:
                while True:
                    choice = input("Approve? [y]es/[n]o/[a]ll/[e]dit/[q]uit: ").strip().lower()
                    if choice in ("y", "yes"):
                        break
                    if choice in ("a", "all"):
                        approve_all = True
                        self._approve_all_for_run = True
                        break
                    if choice in ("n", "no"):
                        results.append(
                            {
                                "call_id": call_id,
                                "name": name,
                                "success": False,
                                "output": None,
                                "error": "Rejected by user",
                            }
                        )
                        name = ""
                        break
                    if choice in ("q", "quit"):
                        return None
                    if choice in ("e", "edit"):
                        edited = input("New arguments (JSON): ").strip()
                        if edited:
                            try:
                                new_args = json.loads(edited)
                            except json.JSONDecodeError as e:
                                self._print(_style(f"Invalid JSON: {e}", _C.YELLOW, enabled=self._color))
                                continue
                            if not isinstance(new_args, dict):
                                self._print(_style("Arguments must be a JSON object.", _C.YELLOW, enabled=self._color))
                                continue
                            args = new_args
                            tc["arguments"] = args
                            self._print(_style("Updated args.", _C.DIM, enabled=self._color))
                        continue

                    self._print("Enter y/n/a/e/q.")

            if not name:
                continue

            # Additional confirmation for shell execution.
            if name == "execute_command":
                confirm = input("Type 'run' to execute this command: ").strip().lower()
                if confirm != "run":
                    results.append(
                        {
                            "call_id": call_id,
                            "name": name,
                            "success": False,
                            "output": None,
                            "error": "Rejected by user",
                        }
                    )
                    continue

            single = {"name": name, "arguments": args, "call_id": call_id}
            out = self._tool_runner.execute(tool_calls=[single])
            results.extend(out.get("results") or [])

        return {"mode": "executed", "results": results}


def _truncate_json(value: Any, *, max_str: int = 800, max_list: int = 50, max_dict: int = 50) -> Any:
    if isinstance(value, str):
        if len(value) <= max_str:
            return value
        head = value[:400]
        tail = value[-200:] if len(value) > 600 else ""
        suffix = f"... ({len(value)} chars total)"
        return head + (("\n" + suffix + "\n" + tail) if tail else ("\n" + suffix))

    if isinstance(value, list):
        trimmed = value[:max_list]
        out = [_truncate_json(v, max_str=max_str, max_list=max_list, max_dict=max_dict) for v in trimmed]
        if len(value) > max_list:
            out.append(f"... ({len(value)} items total)")
        return out

    if isinstance(value, dict):
        items = list(value.items())[:max_dict]
        out_dict: Dict[str, Any] = {}
        for k, v in items:
            out_dict[str(k)] = _truncate_json(v, max_str=max_str, max_list=max_list, max_dict=max_dict)
        if len(value) > max_dict:
            out_dict["..."] = f"({len(value)} keys total)"
        return out_dict

    return value
