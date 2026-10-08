"""AbstractCode TUI host-audio bridge — this computer's speaker and microphone.

The terminal client cannot play or record audio itself, so it runs this script
with the Python that has AbstractVoice installed and talks to it over stdin /
stdout, one JSON object per line. The bridge has no speech engine; synthesis and
transcription run on the gateway (`/voice/tts/stream`, `/audio/transcribe`);
this script only moves audio between those endpoints and the host devices,
through AbstractVoice's own helpers:

- playback: `abstractvoice.tts.NonBlockingAudioPlayer` (the player the Assistant
  uses), output devices from `abstractvoice.tts.audio_devices` when present;
- recording: PortAudio through `sounddevice` (AbstractVoice's audio-io
  dependency), 16 kHz mono PCM16 WAV.

Commands (stdin), every one tagged with the client's generation `gen`:
  {"op":"play","gen":1,"b64":"<wav>","device":"","volume":1.0}
  {"op":"end","gen":1}            -> {"event":"done","gen":1} once drained
  {"op":"stop","gen":1}           -> {"event":"stopped","gen":1}
  {"op":"devices","gen":2}        -> {"event":"devices","gen":2,"output":[...],"input":[...]}
  {"op":"tone","gen":3,...}       -> {"event":"tone_done","gen":3}
  {"op":"record","gen":4,"device":"","gain":1.0,"path":"/tmp/x.wav","max_s":120}
                                  -> {"event":"level","gen":4,"rms":0.1} ~10/s
  {"op":"record_stop","gen":4}    -> {"event":"recorded","gen":4,"path":...,"duration_ms":...,"peak":...}
Events (stdout): ready, fatal, started (first audio of a generation reached the
player), done, stopped, devices, tone_done, level, recorded, error.

`--null-output DIR` replaces the devices with files (tests, machines without
audio): played segments are written to DIR and "played" in real time; a
recording is a 1 s 440 Hz tone.
"""

from __future__ import annotations

import base64
import io
import json
import os
import sys
import threading
import time
import wave

_LOCK = threading.Lock()


def emit(obj: dict) -> None:
    with _LOCK:
        sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
        sys.stdout.flush()


NULL_DIR = ""
if "--null-output" in sys.argv:
    NULL_DIR = sys.argv[sys.argv.index("--null-output") + 1]
    os.makedirs(NULL_DIR, exist_ok=True)

try:
    import numpy as np
    from abstractvoice.tts import NonBlockingAudioPlayer

    try:
        from abstractvoice import __version__ as AV_VERSION
    except Exception:
        AV_VERSION = ""
    sd = None
    if not NULL_DIR:
        import sounddevice as sd  # noqa: F811
except Exception as exc:  # the client turns this into one sentence
    emit({"event": "fatal", "message": f"{type(exc).__name__}: {exc}"})
    sys.exit(3)

try:
    from abstractvoice.tts.audio_devices import list_output_devices
except Exception:  # older AbstractVoice: PortAudio's own list
    list_output_devices = None

SILENT_LEVEL = 0.02
RECORD_RATE = 16000


def decode_wav(data: bytes):
    with wave.open(io.BytesIO(data), "rb") as w:
        rate = w.getframerate()
        channels = w.getnchannels()
        width = w.getsampwidth()
        frames = w.readframes(w.getnframes())
    if width == 2:
        audio = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    elif width == 4:
        audio = np.frombuffer(frames, dtype="<i4").astype(np.float32) / 2147483648.0
    elif width == 1:
        audio = (np.frombuffer(frames, dtype=np.uint8).astype(np.float32) - 128.0) / 128.0
    else:
        raise ValueError(f"unsupported WAV sample width {width}")
    if channels > 1:
        audio = audio.reshape(-1, channels).mean(axis=1)
    return audio.astype(np.float32), int(rate)


def write_wav(path: str, audio, rate: int) -> None:
    pcm = (np.clip(audio, -1.0, 1.0) * 32767.0).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


class Playback:
    """One NonBlockingAudioPlayer per output device; generations gate events."""

    def __init__(self) -> None:
        self.player = None
        self.device = None
        self.gen = 0
        self.ended = False
        self.started = False
        self.null_until = 0.0
        self.null_count = 0

    def _player_for(self, device: str):
        if NULL_DIR:
            return None
        if self.player is not None and self.device == device:
            return self.player
        if self.player is not None:
            try:
                self.player.cleanup()
            except Exception:
                pass
        try:
            player = NonBlockingAudioPlayer(sample_rate=48000, output_device=device or None)
        except TypeError:  # AbstractVoice before output-device support
            if device:
                raise RuntimeError("this AbstractVoice cannot choose the output device; pick System default")
            player = NonBlockingAudioPlayer(sample_rate=48000)
        player.on_audio_start = self._on_start
        player.playback_complete_callback = self._on_drained
        self.player, self.device = player, device
        return player

    def _on_start(self) -> None:
        if self.gen and not self.started:
            self.started = True
            emit({"event": "started", "gen": self.gen})

    def _on_drained(self) -> None:
        # A starved queue between two segments also drains: only the end of
        # the utterance ("end" received) is the end of playback.
        if self.gen and self.ended:
            gen, self.gen = self.gen, 0
            emit({"event": "done", "gen": gen})

    def play(self, gen: int, b64: str, device: str, volume: float) -> None:
        if gen != self.gen:
            self.stop(self.gen, quiet=True)
            self.gen, self.ended, self.started = gen, False, False
        audio, rate = decode_wav(base64.b64decode(b64))
        audio = audio * max(0.0, min(1.0, float(volume)))
        if NULL_DIR:
            self.null_count += 1
            write_wav(os.path.join(NULL_DIR, f"seg-{gen}-{self.null_count}.wav"), audio, rate)
            now = time.monotonic()
            self.null_until = max(self.null_until, now) + len(audio) / float(rate)
            self._on_start()
            return
        self._player_for(device).play_audio(audio, sample_rate=rate)

    def end(self, gen: int) -> None:
        if gen != self.gen:
            emit({"event": "done", "gen": gen})
            return
        self.ended = True
        if NULL_DIR:
            delay = max(0.0, self.null_until - time.monotonic())
            threading.Timer(delay, self._on_drained).start()
            return
        player = self.player
        if player is None or (not player.is_playing and player.audio_queue.empty()):
            self._on_drained()

    def stop(self, gen: int, quiet: bool = False) -> None:
        if self.player is not None:
            try:
                self.player.clear_queue()
            except Exception:
                pass
        self.null_until = 0.0
        self.gen, self.ended = 0, False
        if not quiet:
            emit({"event": "stopped", "gen": gen})

    def tone(self, gen: int, device: str, volume: float) -> None:
        rate = 48000
        t = np.arange(int(rate * 0.18)) / rate
        notes = [np.sin(2 * np.pi * f * t) for f in (660.0, 880.0)]
        fade = np.minimum(1.0, np.minimum(t, t[::-1]) / 0.02)
        audio = np.concatenate([n * fade * 0.4 for n in notes]).astype(np.float32)
        audio = audio * max(0.0, min(1.0, float(volume)))
        if not NULL_DIR:
            self._player_for(device).play_audio(audio, sample_rate=rate)
        threading.Timer(len(audio) / rate + 0.1, lambda: emit({"event": "tone_done", "gen": gen})).start()


def input_index(name: str):
    if not name:
        return None
    for i, d in enumerate(sd.query_devices()):
        if d.get("max_input_channels", 0) > 0 and d.get("name") == name:
            return i
    raise LookupError("The chosen microphone is not connected. Pick another one in Settings → Voice.")


def devices(gen: int) -> None:
    if NULL_DIR:
        emit({"event": "devices", "gen": gen, "output": [{"id": "null-speaker", "name": "Null speaker"}],
              "input": [{"id": "Null microphone", "name": "Null microphone"}]})
        return
    outputs = []
    if list_output_devices is not None:
        outputs = [{"id": d.key, "name": d.name} for d in list_output_devices()]
    else:
        outputs = [{"id": d["name"], "name": d["name"]} for d in sd.query_devices() if d.get("max_output_channels", 0) > 0]
    inputs = [{"id": d["name"], "name": d["name"]} for d in sd.query_devices() if d.get("max_input_channels", 0) > 0]
    emit({"event": "devices", "gen": gen, "output": outputs, "input": inputs})


class Recorder:
    def __init__(self) -> None:
        self.stream = None
        self.gen = 0
        self.path = ""
        self.chunks = []
        self.peak = 0.0
        self.gain = 1.0
        self.started = 0.0
        self.last_level = 0.0
        self.timer = None

    def start(self, gen: int, device: str, gain: float, path: str, max_s: float) -> None:
        self.cancel()
        self.gen, self.path, self.chunks, self.peak = gen, path, [], 0.0
        self.gain = float(gain or 1.0)
        self.started = time.monotonic()
        if NULL_DIR:
            t = np.arange(RECORD_RATE) / RECORD_RATE
            self.chunks = [(0.3 * np.sin(2 * np.pi * 440.0 * t)).astype(np.float32)]
            self.peak = 0.3
            emit({"event": "level", "gen": gen, "rms": 0.21})
        else:
            index = input_index(device)

            def callback(indata, frames, _time, status):
                mono = indata[:, 0] * self.gain
                self.chunks.append(mono.copy())
                level = float(np.sqrt(np.mean(mono * mono))) if len(mono) else 0.0
                self.peak = max(self.peak, float(np.max(np.abs(mono))) if len(mono) else 0.0)
                now = time.monotonic()
                if now - self.last_level >= 0.1:
                    self.last_level = now
                    emit({"event": "level", "gen": gen, "rms": round(min(1.0, level * 4.0), 3)})

            self.stream = sd.InputStream(device=index, channels=1, samplerate=RECORD_RATE, dtype="float32", callback=callback)
            self.stream.start()
        if max_s and max_s > 0:
            self.timer = threading.Timer(float(max_s), lambda: self.stop(gen))
            self.timer.daemon = True
            self.timer.start()

    def cancel(self) -> None:
        if self.timer is not None:
            self.timer.cancel()
            self.timer = None
        if self.stream is not None:
            try:
                self.stream.stop()
                self.stream.close()
            except Exception:
                pass
            self.stream = None

    def stop(self, gen: int) -> None:
        if gen != self.gen:
            return
        self.cancel()
        self.gen = 0
        audio = np.concatenate(self.chunks) if self.chunks else np.zeros(0, dtype=np.float32)
        write_wav(self.path, audio, RECORD_RATE)
        emit({"event": "recorded", "gen": gen, "path": self.path,
              "duration_ms": int(1000 * len(audio) / RECORD_RATE), "peak": round(self.peak, 4)})


def main() -> None:
    playback = Playback()
    recorder = Recorder()
    emit({"event": "ready", "abstractvoice": AV_VERSION, "null": bool(NULL_DIR)})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            cmd = json.loads(line)
        except Exception:
            emit({"event": "error", "op": "", "gen": 0, "message": "invalid command line"})
            continue
        op = str(cmd.get("op") or "")
        gen = int(cmd.get("gen") or 0)
        try:
            if op == "play":
                playback.play(gen, str(cmd.get("b64") or ""), str(cmd.get("device") or ""), float(cmd.get("volume", 1.0)))
            elif op == "end":
                playback.end(gen)
            elif op == "stop":
                playback.stop(gen)
            elif op == "devices":
                devices(gen)
            elif op == "tone":
                playback.tone(gen, str(cmd.get("device") or ""), float(cmd.get("volume", 1.0)))
            elif op == "record":
                recorder.start(gen, str(cmd.get("device") or ""), float(cmd.get("gain", 1.0)), str(cmd["path"]), float(cmd.get("max_s", 120)))
            elif op == "record_stop":
                recorder.stop(gen)
            elif op == "quit":
                break
            else:
                emit({"event": "error", "op": op, "gen": gen, "message": f"unknown op {op!r}"})
        except Exception as exc:
            if op == "record":
                recorder.cancel()
                recorder.gen = 0
            message = str(exc) if isinstance(exc, LookupError) else f"{type(exc).__name__}: {exc}"
            emit({"event": "error", "op": op, "gen": gen, "message": message.strip("'\"")})
    recorder.cancel()
    playback.stop(0, quiet=True)


if __name__ == "__main__":
    main()
