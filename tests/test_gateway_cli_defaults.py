from __future__ import annotations


def test_default_gateway_url_defaults_to_8081(monkeypatch) -> None:
    from abstractcode.gateway_cli import default_gateway_url

    for k in ("ABSTRACTCODE_GATEWAY_URL", "ABSTRACTFLOW_GATEWAY_URL", "ABSTRACTGATEWAY_URL"):
        monkeypatch.delenv(k, raising=False)

    assert default_gateway_url() == "http://127.0.0.1:8081"


def test_default_gateway_url_prefers_explicit_env(monkeypatch) -> None:
    from abstractcode.gateway_cli import default_gateway_url

    monkeypatch.setenv("ABSTRACTGATEWAY_URL", "http://example:9999/")
    monkeypatch.delenv("ABSTRACTCODE_GATEWAY_URL", raising=False)
    monkeypatch.delenv("ABSTRACTFLOW_GATEWAY_URL", raising=False)

    assert default_gateway_url() == "http://example:9999"


def test_default_gateway_token_resolves_first_token(monkeypatch) -> None:
    from abstractcode.gateway_cli import default_gateway_token

    for k in (
        "ABSTRACTCODE_GATEWAY_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKEN",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKENS",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKENS",
    ):
        monkeypatch.delenv(k, raising=False)

    monkeypatch.setenv("ABSTRACTGATEWAY_AUTH_TOKENS", "a, b, c")
    assert default_gateway_token() == "a"


def test_default_gateway_token_prefers_single_token_vars(monkeypatch) -> None:
    from abstractcode.gateway_cli import default_gateway_token

    for k in (
        "ABSTRACTCODE_GATEWAY_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKEN",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKENS",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKENS",
    ):
        monkeypatch.delenv(k, raising=False)

    monkeypatch.setenv("ABSTRACTGATEWAY_AUTH_TOKENS", "a, b")
    monkeypatch.setenv("ABSTRACTGATEWAY_AUTH_TOKEN", "t")
    assert default_gateway_token() == "t"
