from __future__ import annotations

import json

import pytest

pytestmark = pytest.mark.basic


def test_query_gateway_kg_command_builds_request_and_formats_triples(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from abstractcode import gateway_cli

    called: dict[str, object] = {}

    def _fake_request_json(*, method: str, url: str, token: str | None, payload=None, timeout_s: float = 30.0):  # noqa: ANN001
        called["method"] = method
        called["url"] = url
        called["token"] = token
        called["payload"] = payload
        called["timeout_s"] = timeout_s
        return {
            "ok": True,
            "scope": "session",
            "owner_id": "session_memory_sess-1",
            "count": 1,
            "items": [
                {
                    "observed_at": "2026-01-21T00:00:00+00:00",
                    "subject": "ex:person-laurent",
                    "predicate": "rdf:type",
                    "object": "schema:person",
                    "scope": "session",
                    "owner_id": "session_memory_sess-1",
                    "confidence": 0.9,
                    "attributes": {"_retrieval": {"score": 0.42}},
                }
            ],
            "warnings": ["hello"],
        }

    monkeypatch.setattr(gateway_cli, "_request_json", _fake_request_json)

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id="sess-1",
        scope="session",
        subject="ex:person-laurent",
        predicate="rdf:type",
        object_value="schema:person",
        since="2026-01-01T00:00:00+00:00",
        until="2026-12-31T00:00:00+00:00",
        active_at="2026-01-21T00:00:00+00:00",
        query_text="laurent",
        min_score=0.1,
        limit=12,
        order="desc",
        fmt="triples",
    )

    out, err = capsys.readouterr()
    assert "warning: hello" in err
    assert "[2026-01-21T00:00:00+00:00] ex:person-laurent --rdf:type--> schema:person" in out
    assert "confidence=0.900" in out
    assert "score=0.420" in out

    assert called["method"] == "POST"
    assert called["url"] == "http://example:8081/api/gateway/kg/query"
    assert called["token"] == "t"
    assert isinstance(called["payload"], dict)
    payload = called["payload"]
    assert payload["session_id"] == "sess-1"
    assert payload["scope"] == "session"
    assert payload["subject"] == "ex:person-laurent"
    assert payload["predicate"] == "rdf:type"
    assert payload["object"] == "schema:person"
    assert payload["since"] == "2026-01-01T00:00:00+00:00"
    assert payload["until"] == "2026-12-31T00:00:00+00:00"
    assert payload["active_at"] == "2026-01-21T00:00:00+00:00"
    assert payload["query_text"] == "laurent"
    assert payload["min_score"] == 0.1
    assert payload["limit"] == 12
    assert payload["order"] == "desc"


def test_query_gateway_kg_command_supports_json_and_jsonl(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from abstractcode import gateway_cli

    def _fake_request_json(*, method: str, url: str, token: str | None, payload=None, timeout_s: float = 30.0):  # noqa: ANN001
        del method, url, token, payload, timeout_s
        return {
            "ok": True,
            "scope": "session",
            "owner_id": "o1",
            "count": 2,
            "items": [
                {"subject": "ex:a", "predicate": "rdf:type", "object": "schema:thing", "scope": "session", "owner_id": "o1"},
                {"subject": "ex:b", "predicate": "rdf:type", "object": "schema:thing", "scope": "session", "owner_id": "o1"},
            ],
        }

    monkeypatch.setattr(gateway_cli, "_request_json", _fake_request_json)

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id="run-1",
        scope="session",
        fmt="json",
        pretty=True,
    )
    out, err = capsys.readouterr()
    assert err.strip() == ""
    parsed = json.loads(out)
    assert parsed["count"] == 2

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id="run-1",
        scope="session",
        fmt="jsonl",
    )
    out2, err2 = capsys.readouterr()
    assert err2.strip() == ""
    lines = [ln for ln in out2.splitlines() if ln.strip()]
    assert len(lines) == 2
    assert json.loads(lines[0])["subject"] == "ex:a"
    assert json.loads(lines[1])["subject"] == "ex:b"


def test_query_gateway_kg_command_all_owners_allows_missing_id(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    from abstractcode import gateway_cli

    called: dict[str, object] = {}

    def _fake_request_json(*, method: str, url: str, token: str | None, payload=None, timeout_s: float = 30.0):  # noqa: ANN001
        called["method"] = method
        called["url"] = url
        called["token"] = token
        called["payload"] = payload
        called["timeout_s"] = timeout_s
        return {"ok": True, "scope": "session", "count": 0, "items": []}

    monkeypatch.setattr(gateway_cli, "_request_json", _fake_request_json)

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id=None,
        scope="session",
        all_owners=True,
        limit=0,
        fmt="json",
    )

    out, err = capsys.readouterr()
    assert err.strip() == ""
    assert json.loads(out)["ok"] is True
    assert isinstance(called["payload"], dict)
    payload = called["payload"]
    assert payload.get("all_owners") is True
    assert "run_id" not in payload


def test_query_gateway_kg_command_uses_session_id_for_scope_session(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from abstractcode import gateway_cli

    calls: list[dict] = []

    def _fake_request_json(*, method: str, url: str, token: str | None, payload=None, timeout_s: float = 30.0):  # noqa: ANN001
        del method, timeout_s
        assert url.endswith("/api/gateway/kg/query")
        assert token == "t"
        assert isinstance(payload, dict)
        calls.append(dict(payload))
        return {
            "ok": True,
            "scope": "session",
            "owner_id": "session_memory_sess-uuid",
            "count": 0,
            "items": [],
        }

    monkeypatch.setattr(gateway_cli, "_request_json", _fake_request_json)

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id="sess-uuid",
        scope="session",
        fmt="json",
    )

    out, err = capsys.readouterr()
    assert json.loads(out)["ok"] is True
    assert err.strip() == ""
    assert len(calls) == 1
    assert calls[0].get("session_id") == "sess-uuid"
    assert "run_id" not in calls[0]


def test_query_gateway_kg_command_retries_with_session_id_for_scope_all_on_run_not_found(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    from abstractcode import gateway_cli

    calls: list[dict] = []

    def _fake_request_json(*, method: str, url: str, token: str | None, payload=None, timeout_s: float = 30.0):  # noqa: ANN001
        del method, timeout_s
        assert url.endswith("/api/gateway/kg/query")
        assert token == "t"
        assert isinstance(payload, dict)
        calls.append(dict(payload))
        if payload.get("scope") == "all" and payload.get("run_id") == "sess-uuid" and "session_id" not in payload:
            raise RuntimeError("Gateway HTTP 404: {\"detail\":\"Run 'sess-uuid' not found\"}")
        return {
            "ok": True,
            "scope": "all",
            "count": 0,
            "items": [],
            "warnings": ["run scope omitted (no run_id available)"],
        }

    monkeypatch.setattr(gateway_cli, "_request_json", _fake_request_json)

    gateway_cli.query_gateway_kg_command(
        gateway_url="http://example:8081",
        gateway_token="t",
        run_id="sess-uuid",
        scope="all",
        fmt="json",
    )

    out, err = capsys.readouterr()
    assert json.loads(out)["ok"] is True
    assert "warning: run scope omitted (no run_id available)" in err
    assert len(calls) == 2
    assert calls[0].get("scope") == "all"
    assert calls[1].get("scope") == "all"
    assert calls[1].get("session_id") == "sess-uuid"
