import pytest


def test_flow_cli_input_parsing():
    from abstractcode.flow_cli import _parse_kv_list, _parse_unknown_params, _render_text

    assert _render_text("a\\n\\nb") == "a\n\nb"

    unknown = _parse_unknown_params(
        [
            "--query",
            "who are you?",
            "--max_web_search",
            "15",
            "--follow_up_questions=true",
            "--flag",
            "x=1",
        ]
    )
    assert unknown["query"] == "who are you?"
    assert unknown["max_web_search"] == 15
    assert unknown["follow_up_questions"] is True
    assert unknown["flag"] is True
    assert unknown["x"] == 1

    kv = _parse_kv_list(["a=1", "b=true", 'c={"x":2}', "d=0"])
    assert kv == {"a": 1, "b": True, "c": {"x": 2}, "d": 0}


def test_flow_cli_tool_approve_all_persists_for_run():
    from abstractcode.flow_cli import _ApprovalState, _approve_and_execute

    class FakeToolRunner:
        def __init__(self):
            self.calls = []

        def execute(self, *, tool_calls):
            self.calls.append(list(tool_calls))
            results = []
            for tc in tool_calls:
                results.append(
                    {
                        "call_id": tc.get("call_id", ""),
                        "name": tc.get("name", ""),
                        "success": True,
                        "output": {"ok": True},
                        "error": None,
                    }
                )
            return {"mode": "executed", "results": results}

    runner = FakeToolRunner()
    approval = _ApprovalState()

    prompts = []

    def prompt_fn(msg: str) -> str:
        prompts.append(msg)
        return "a"

    payload1 = _approve_and_execute(
        tool_calls=[{"name": "web_search", "arguments": {"query": "x"}, "call_id": "1"}],
        tool_runner=runner,
        auto_approve=False,
        approval_state=approval,
        prompt_fn=prompt_fn,
        print_fn=lambda _: None,
    )
    assert payload1 is not None
    assert approval.approve_all is True
    assert len(prompts) == 1

    # Second approval should not prompt again.
    payload2 = _approve_and_execute(
        tool_calls=[{"name": "web_search", "arguments": {"query": "y"}, "call_id": "2"}],
        tool_runner=runner,
        auto_approve=False,
        approval_state=approval,
        prompt_fn=lambda _: pytest.fail("prompt_fn called after approve-all"),
        print_fn=lambda _: None,
    )
    assert payload2 is not None
    assert len(runner.calls) == 2
