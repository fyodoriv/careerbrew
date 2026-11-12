#!/usr/bin/env bash
# US-57: practice by voice; the answer is transcribed on this machine and graded by the rubric; typing is the fallback.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
VOICE="$FIX/voice"
out="$(cb drill speak beh-impact-1 --audio "$VOICE/behavioral-answer.wav" 2>&1 || true)"
expect "$out" "no local speech-to-text engine" "error case: no engine names the fix"
expect "$out" "--text" "error case: the typed fallback is offered"
cb config set stt_command "$VOICE/fake-stt.sh {audio}" >/dev/null
out="$(cb drill speak beh-impact-1 --audio "$VOICE/behavioral-answer.wav")"
expect "$out" "Question: " "the question is asked"
expect "$out" "Transcript: The situation was" "the spoken answer is transcribed locally"
expect "$out" "Met: ownership, result, star" "the transcript is graded by the rubric"
expect "$out" "Practice: Named a trade-off" "missed criteria are named"
out="$(cb drill speak arrays-two-pointers-1 --text "Clarify edge cases first. Brute force is O(n^2), two pointers is O(n).")"
expect "$out" "Met: approach, clarify, complexity" "typed fallback grades the same way"
refute "$out" "Transcript:" "a typed answer is not echoed as a transcript"
if data_has "checkout page"; then fail "the transcript was stored"; fi
event_seen tutor.voice
echo "ok US-57 voice practice"
