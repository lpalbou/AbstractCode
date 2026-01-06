from __future__ import annotations

from abstractcode.react_shell import ReactShell


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


def _minimal_shell() -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    return shell


def test_write_file_summary_does_not_inject_current_folder() -> None:
    shell = _minimal_shell()

    ReactShell._print_tool_observation(
        shell,
        tool_name="write_file",
        raw="✅ Successfully written to '/abs/demo.txt' (5 bytes, 1 lines)",
        ok=True,
        tool_args={"file_path": "demo.txt", "content": "hello"},
    )

    assert any("/abs/demo.txt" in line for line in shell._output_lines)
    assert not any("current folder:" in line for line in shell._output_lines)


def test_read_file_summary_does_not_inject_current_folder() -> None:
    shell = _minimal_shell()

    ReactShell._print_tool_observation(
        shell,
        tool_name="read_file",
        raw="File: /abs/demo.txt (1 lines)\n\n1: hello",
        ok=True,
        tool_args={"file_path": "demo.txt"},
    )

    assert any("✅ Read" in line for line in shell._output_lines)
    assert any("/abs/demo.txt" in line for line in shell._output_lines)
    assert not any("current folder:" in line for line in shell._output_lines)

