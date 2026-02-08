from __future__ import annotations

import hashlib
import os
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Protocol, Tuple


class _AbstractCoreLLMClient(Protocol):
    def generate(
        self,
        *,
        prompt: str,
        messages: Optional[List[Dict[str, str]]] = None,
        system_prompt: Optional[str] = None,
        tools: Optional[List[Dict[str, Any]]] = None,
        media: Optional[List[Any]] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]: ...

    def get_model_capabilities(self) -> Dict[str, Any]: ...


_AUDIO_EXTS = {
    ".wav",
    ".mp3",
    ".m4a",
    ".aac",
    ".flac",
    ".ogg",
    ".opus",
    ".wma",
    ".aiff",
    ".aif",
    ".caf",
    ".webm",
}


def _normalize_audio_policy(raw: Any) -> str:
    s = str(raw or "").strip().lower()
    if not s:
        return ""
    if s in {"native"}:
        return "native_only"
    if s in {"stt"}:
        return "speech_to_text"
    return s


def _config_audio_strategy() -> str:
    try:
        from abstractcore.config.manager import get_config_manager  # type: ignore

        cfg = get_config_manager().config
        audio = getattr(cfg, "audio", None)
        return _normalize_audio_policy(getattr(audio, "strategy", None))
    except Exception:
        return ""


def _config_stt_language() -> Optional[str]:
    try:
        from abstractcore.config.manager import get_config_manager  # type: ignore

        cfg = get_config_manager().config
        audio = getattr(cfg, "audio", None)
        lang = getattr(audio, "stt_language", None)
        s = str(lang or "").strip()
        return s or None
    except Exception:
        return None


def _effective_audio_policy(params: Optional[Dict[str, Any]]) -> str:
    # Match AbstractCore precedence: per-call kwarg > config default > native_only.
    raw = None
    if isinstance(params, dict):
        raw = params.get("audio_policy")
        if raw is None:
            raw = params.get("audio_handling_policy")
    pol = _normalize_audio_policy(raw)
    if pol:
        return pol
    pol = _config_audio_strategy()
    return pol or "native_only"


def _effective_stt_language(params: Optional[Dict[str, Any]]) -> Optional[str]:
    if isinstance(params, dict):
        raw = params.get("stt_language")
        if raw is None:
            raw = params.get("audio_language")
        s = str(raw or "").strip()
        if s:
            return s
    return _config_stt_language()


def _extract_audio_paths(media: Optional[List[Any]]) -> Tuple[List[str], List[Any]]:
    audio: List[str] = []
    remaining: List[Any] = []
    items = list(media or [])
    for it in items:
        if isinstance(it, str):
            p = it.strip()
            if p and Path(p).suffix.lower() in _AUDIO_EXTS:
                audio.append(p)
            else:
                remaining.append(it)
            continue
        remaining.append(it)
    return audio, remaining


def _sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _inject_audio_context_into_prompt(prompt: str, *, audio_lines: List[str]) -> str:
    original = str(prompt or "").strip()
    parts: List[str] = []
    parts.append(
        "Audio context from attached audio file(s) (treat as directly observed; do not mention this section):"
    )
    parts.extend([ln for ln in audio_lines if str(ln or "").strip()])
    if original:
        parts.append("Now answer the user's request:")
        parts.append(original)
    return "\n\n".join(parts).strip()


def _inject_audio_context_into_messages(
    messages: Optional[List[Dict[str, str]]],
    *,
    audio_lines: List[str],
) -> Tuple[Optional[List[Dict[str, str]]], str]:
    """Return (updated_messages, updated_prompt).

    If messages contain a user turn, replace the last user message content with an injected prompt.
    Otherwise, return an injected prompt string (caller can use it as `prompt=`).
    """
    if not isinstance(messages, list) or not messages:
        return None, _inject_audio_context_into_prompt("", audio_lines=audio_lines)

    out = [dict(m) for m in messages if isinstance(m, dict)]
    for i in range(len(out) - 1, -1, -1):
        m = out[i]
        if str(m.get("role") or "") != "user":
            continue
        base = str(m.get("content") or "")
        out[i] = dict(m, content=_inject_audio_context_into_prompt(base, audio_lines=audio_lines))
        return out, ""

    return out, _inject_audio_context_into_prompt("", audio_lines=audio_lines)


class AudioSttFallbackLLMClient:
    """Wrapper that adds STT fallback for audio attachments at LLM-call time.

    Why:
    - AbstractCode passes attachments to AbstractCore via Runtime artifacts.
    - Some installs may not have AbstractCore's optional capability registry available.
    - We still want `@audio.wav` to work in `abstractcode --prompt` like in AbstractCore CLI.
    """

    def __init__(
        self,
        inner: _AbstractCoreLLMClient,
        *,
        transcribe_file: Optional[Callable[[str, Optional[str]], str]] = None,
    ) -> None:
        self._inner = inner
        self._transcribe_file = transcribe_file
        self._transcript_cache: Dict[str, str] = {}

    def get_model_capabilities(self) -> Dict[str, Any]:
        return dict(self._inner.get_model_capabilities() or {})

    def _transcriber(self) -> Optional[Callable[[str, Optional[str]], str]]:
        if self._transcribe_file is not None:
            return self._transcribe_file
        try:
            from abstractvoice import VoiceManager  # type: ignore
        except Exception:
            return None

        vm = VoiceManager(debug_mode=False, allow_downloads=True)

        def _fn(path: str, language: Optional[str]) -> str:
            return str(vm.transcribe_file(path, language=language) or "")

        self._transcribe_file = _fn
        return self._transcribe_file

    def generate(
        self,
        *,
        prompt: str,
        messages: Optional[List[Dict[str, str]]] = None,
        system_prompt: Optional[str] = None,
        tools: Optional[List[Dict[str, Any]]] = None,
        media: Optional[List[Any]] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        audio_paths, remaining_media = _extract_audio_paths(list(media) if media is not None else None)
        if not audio_paths:
            return self._inner.generate(
                prompt=str(prompt or ""),
                messages=messages,
                system_prompt=system_prompt,
                tools=tools,
                media=media,
                params=params,
            )

        policy = _effective_audio_policy(params)
        caps = self.get_model_capabilities()
        model_supports_audio = bool(caps.get("audio_support", False))

        # Match AbstractCore behavior: auto transcribes only when the model isn't audio-capable.
        if policy in {"native_only", "native", "disabled"}:
            return self._inner.generate(
                prompt=str(prompt or ""),
                messages=messages,
                system_prompt=system_prompt,
                tools=tools,
                media=media,
                params=params,
            )

        if policy == "auto" and model_supports_audio:
            return self._inner.generate(
                prompt=str(prompt or ""),
                messages=messages,
                system_prompt=system_prompt,
                tools=tools,
                media=media,
                params=params,
            )

        transcribe = self._transcriber()
        if transcribe is None:
            return self._inner.generate(
                prompt=str(prompt or ""),
                messages=messages,
                system_prompt=system_prompt,
                tools=tools,
                media=media,
                params=params,
            )

        stt_language = _effective_stt_language(params)
        audio_lines: List[str] = []
        for i, path in enumerate(audio_paths):
            name = Path(path).name or f"audio_{i+1}"
            sha = ""
            try:
                sha = _sha256_file(path)
            except Exception:
                sha = ""
            cached = self._transcript_cache.get(sha) if sha else None
            if cached is None:
                try:
                    cached = str(transcribe(path, stt_language) or "").strip()
                except Exception:
                    cached = ""
                if sha:
                    self._transcript_cache[sha] = cached
            audio_lines.append(f"Audio {i+1} ({name}): {cached}".rstrip())

        new_prompt = str(prompt or "")
        new_messages = messages

        if str(new_prompt or "").strip():
            new_prompt = _inject_audio_context_into_prompt(new_prompt, audio_lines=audio_lines)
        else:
            new_messages, injected_prompt = _inject_audio_context_into_messages(messages, audio_lines=audio_lines)
            if injected_prompt:
                new_prompt = injected_prompt

        # Remove audio from media since we injected transcript text context.
        return self._inner.generate(
            prompt=str(new_prompt or ""),
            messages=new_messages,
            system_prompt=system_prompt,
            tools=tools,
            media=remaining_media or None,
            params=params,
        )

    def __getattr__(self, name: str) -> Any:
        return getattr(self._inner, name)

