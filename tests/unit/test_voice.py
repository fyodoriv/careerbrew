"""Voice practice (#56): local speech-to-text, rubric grading, typed fallback. Synthetic fixtures only."""

import base64

import pytest

from careerbrew import config
from careerbrew.tutor import engine, voice

from .conftest import FIXTURES

VOICE = FIXTURES / "voice"
# behavioral-answer.wav is generated silence (0.1 s, mono, 16-bit, 16 kHz), not a recording. Recreate it with Python:
# w = wave.open(path, "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000); w.writeframes(bytes(3200)); w.close()
FAKE = f"{VOICE / 'fake-stt.sh'} {{audio}}"


def test_spoken_answer_is_transcribed_locally_and_graded_by_the_rubric(home):
    config.set_value("stt_command", FAKE)
    out = voice.answer("beh-impact-1", audio=VOICE / "behavioral-answer.wav")
    assert out["source"] == "audio" and "[00:" not in out["transcript"], "timestamps are stripped"
    assert out["row"]["met"] == ["ownership", "result", "star"]
    assert out["row"]["graded_by"] == "voice-audio"
    assert engine.attempts()[-1]["item"] == "beh-impact-1"
    assert "transcript" not in engine.attempts()[-1], "the transcript is not stored"


def test_typed_fallback_grades_the_same_way(home):
    out = voice.answer("arrays-two-pointers-1", text="Let me clarify edge cases. Brute force is O(n^2); two pointers is O(n) time, constant space.")
    assert set(out["row"]["met"]) == {"clarify", "approach", "complexity"}
    assert out["source"] == "typed"


def test_cue_with_symbol_matches_after_a_number(home):
    assert "result" in voice.grade("beh-impact-1", "we saved 40% of the time")


def test_no_engine_names_the_fix(home, monkeypatch):
    monkeypatch.setattr(voice.shutil, "which", lambda _n: None)
    with pytest.raises(voice.VoiceError, match="stt_model"):
        voice.answer("beh-impact-1", audio=VOICE / "behavioral-answer.wav")
    with pytest.raises(voice.VoiceError, match="give an answer"):
        voice.answer("beh-impact-1", text="  ")


def test_stt_command_needs_the_audio_placeholder(home):
    with pytest.raises(ValueError, match="placeholder"):
        config.set_value("stt_command", "whisper-cli -f")


def test_web_api_grades_base64_audio_and_typed_answers(home):
    from careerbrew import server

    config.set_value("stt_command", FAKE)
    assert server.drill_view()["stt"] is True and server.drill_view()["criteria"]
    raw = base64.b64encode((VOICE / "behavioral-answer.wav").read_bytes()).decode()
    out = server._drill_voice({"item": "beh-impact-1", "audio": raw, "mime": "audio/wav"})
    assert out["source"] == "audio" and out["row"]["met"] == ["ownership", "result", "star"]
    out = server._drill_voice({"item": "beh-impact-1", "text": "I decided and we cut cost by 30%"})
    assert set(out["row"]["met"]) >= {"ownership", "result"}
    with pytest.raises(server.ApiError):
        server._drill_voice({"item": "beh-impact-1", "audio": "not base64!!", "mime": "audio/wav"})
    config.set_value("stt_command", "false {audio}")
    with pytest.raises(server.ApiError, match="speech-to-text failed"):
        server._drill_voice({"item": "beh-impact-1", "audio": raw, "mime": "audio/wav"})


def test_voice_api_is_personal_only(home):
    from careerbrew import server

    assert any("/api/drill/voice".startswith(p) for p in server.JOB_PATHS)
