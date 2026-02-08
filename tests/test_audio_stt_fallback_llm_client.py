from __future__ import annotations

from pathlib import Path


def test_audio_stt_fallback_injects_into_last_user_message(tmp_path: Path) -> None:
    from abstractcode.media_fallbacks import AudioSttFallbackLLMClient

    audio = tmp_path / "always.wav"
    audio.write_bytes(b"wav")
    image = tmp_path / "test.jpg"
    image.write_bytes(b"jpg")

    calls = {}

    class DummyInner:
        def get_model_capabilities(self):
            return {"audio_support": False}

        def generate(self, *, prompt, messages=None, system_prompt=None, tools=None, media=None, params=None):
            calls["prompt"] = prompt
            calls["messages"] = messages
            calls["media"] = media
            calls["params"] = params
            return {"content": "ok"}

    def fake_transcribe(path: str, language: str | None):
        assert Path(path).name == "always.wav"
        assert language is None
        return "hello world"

    client = AudioSttFallbackLLMClient(DummyInner(), transcribe_file=fake_transcribe)

    messages = [{"role": "user", "content": "what does it say?"}]
    client.generate(
        prompt="",
        messages=messages,
        system_prompt=None,
        tools=None,
        media=[str(audio), str(image)],
        params={"audio_policy": "auto"},
    )

    # Prompt stays empty in message-based calls; injection happens into the last user turn.
    assert calls["prompt"] == ""
    out_messages = calls["messages"]
    assert isinstance(out_messages, list)
    assert out_messages[-1]["role"] == "user"
    content = out_messages[-1]["content"]
    assert "Audio context from attached audio file(s)" in content
    assert "Audio 1 (always.wav): hello world" in content
    assert "Now answer the user's request:" in content
    assert content.strip().endswith("what does it say?")

    # Audio is removed from media after transcription; other media stays.
    assert calls["media"] == [str(image)]


def test_audio_stt_fallback_respects_native_only(tmp_path: Path) -> None:
    from abstractcode.media_fallbacks import AudioSttFallbackLLMClient

    audio = tmp_path / "always.wav"
    audio.write_bytes(b"wav")

    calls = {}

    class DummyInner:
        def get_model_capabilities(self):
            return {"audio_support": False}

        def generate(self, *, prompt, messages=None, system_prompt=None, tools=None, media=None, params=None):
            calls["prompt"] = prompt
            calls["messages"] = messages
            calls["media"] = media
            calls["params"] = params
            return {"content": "ok"}

    client = AudioSttFallbackLLMClient(DummyInner(), transcribe_file=lambda _p, _l: "ignored")

    client.generate(
        prompt="what is this?",
        messages=None,
        system_prompt=None,
        tools=None,
        media=[str(audio)],
        params={"audio_policy": "native_only"},
    )

    assert calls["prompt"] == "what is this?"
    assert calls["media"] == [str(audio)]

