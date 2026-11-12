"""Voice practice: transcribe a spoken answer on this machine, then grade it by the rubric.

- Speech-to-text runs locally only. The engine is the `stt_command` setting: a command
  with an `{audio}` placeholder that prints the transcript to stdout (for example
  whisper.cpp: `whisper-cli -nt -np -m ~/models/ggml-base.en.bin -f {audio}`).
  With no setting, careerbrew uses `whisper-cli` when it is on PATH and `stt_model` is set.
- Audio that is not WAV is converted to 16 kHz mono WAV with ffmpeg when it is installed.
- No engine? Type the answer instead (`--text`); grading is the same.
- Grading is deterministic: a criterion is met when the transcript names one of its cues
  (`cues:` in the rubric file, else the long words of the criterion text).
- The audio and the transcript are not stored; only the attempt row (criteria met) is.

Lifecycle: no state of its own; audio goes to a temp dir that is removed after each call.
"""

from __future__ import annotations

import re
import shlex
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any

from .. import config, events
from . import engine

STOP = {"about", "after", "their", "there", "these", "which", "would", "could", "should", "before", "least", "where", "while", "named"}


class VoiceError(RuntimeError):
    pass


def stt_command() -> str:
    cmd = str(config.get("stt_command") or "")
    if cmd:
        return cmd
    model = str(config.get("stt_model") or "")
    if model and shutil.which("whisper-cli"):
        return f"whisper-cli -nt -np -m {shlex.quote(str(Path(model).expanduser()))} -f {{audio}}"
    return ""


def _to_wav(src: Path, tmp: Path) -> Path:
    if src.suffix.lower() == ".wav" or not shutil.which("ffmpeg"):
        return src
    out = tmp / "answer.wav"
    done = subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(src), "-ar", "16000", "-ac", "1", str(out)], capture_output=True, timeout=120)
    return out if done.returncode == 0 else src  # the engine may still read the original format


def transcribe(audio: Path) -> str:
    """Run the local speech-to-text command on one audio file and return the text."""
    cmd = stt_command()
    if not cmd:
        raise VoiceError("no local speech-to-text engine: install whisper.cpp and run `careerbrew config set stt_model <path to ggml model>`, or type your answer with --text")
    if not audio.is_file():
        raise VoiceError(f"no audio file at {audio}")
    with tempfile.TemporaryDirectory(prefix="careerbrew-voice-") as tmp:
        wav = _to_wav(audio, Path(tmp))
        argv = [part.replace("{audio}", str(wav)) for part in shlex.split(cmd)]
        try:
            done = subprocess.run(argv, capture_output=True, text=True, timeout=300)
        except FileNotFoundError:
            raise VoiceError(f"speech-to-text command not found: {argv[0]} (check `careerbrew config`)") from None
    if done.returncode != 0:
        raise VoiceError(f"speech-to-text failed (exit {done.returncode}); run the stt_command by hand to see why")
    text = " ".join(re.sub(r"\[[^\]]*\]", " ", done.stdout).split())
    if not text:
        raise VoiceError("the transcript is empty; speak closer to the microphone, or type your answer")
    return text


def cues(criterion: dict[str, Any]) -> list[str]:
    if criterion.get("cues"):
        return [str(c).lower() for c in criterion["cues"]]
    return [w for w in re.findall(r"[a-z][a-z-]{4,}", str(criterion["text"]).lower()) if w not in STOP]


def grade(item_id: str, text: str) -> list[str]:
    """Return the ids of the rubric criteria the answer meets."""
    items = engine.items()
    if item_id not in items:
        raise KeyError(f"unknown item {item_id} (see `careerbrew drill list`)")
    rubric = engine.rubrics()[items[item_id]["rubric"]]
    said = " ".join(re.findall(r"[a-z0-9%()^'-]+", text.lower()))
    return [c["id"] for c in rubric["criteria"] if any(_says(said, cue) for cue in cues(c))]


def _says(said: str, cue: str) -> bool:
    pre = r"(?<![a-z0-9])" if cue[:1].isalnum() else ""
    post = r"(?![a-z0-9])" if cue[-1:].isalnum() else ""
    return re.search(pre + re.escape(cue) + post, said) is not None


def answer(item_id: str, text: str | None = None, audio: Path | None = None, seconds: int | None = None) -> dict[str, Any]:
    """Grade one spoken (audio) or typed (text) answer and record the attempt."""
    if audio is not None:
        text, source = transcribe(audio), "audio"
    elif text and text.strip():
        source = "typed"
    else:
        raise VoiceError("give an answer: --audio <file> or --text <answer>")
    met = grade(item_id, text)
    row = engine.record(item_id, met, seconds=seconds, graded_by=f"voice-{source}")
    events.emit("tutor.voice", status=source, words=len(text.split()), score=row["score"])
    return {"transcript": text, "source": source, "row": row, "feedback": engine.feedback(row)}
