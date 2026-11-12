#!/bin/sh
# Synthetic local speech-to-text engine for tests. Prints the transcript stored next to
# the audio file, or the fixture behavioral answer for uploads (the web app's temp file).
side="${1%.*}.txt"
if [ -f "$side" ]; then cat "$side"; else cat "$(dirname "$0")/behavioral-answer.txt"; fi
