#!/usr/bin/env bash
# US-43: data files are ciphertext on disk, careerbrew still reads them, and a wrong key is a clear error.
source "$(dirname "$0")/lib.sh"
seed_jobs   # writes plaintext files by hand, like a user before encryption
on_disk() { grep -rqaF "$1" --exclude-dir=logs --exclude-dir=out "$CAREERBREW_HOME"; }
cb work note "Shipped PROJECTZEBRA" --tags delivery >/dev/null
if on_disk PROJECTZEBRA; then fail "a file careerbrew wrote is plaintext on disk"; fi
out="$(cb data status)"; expect "$out" "still plaintext" "hand-written files are reported"
out="$(cb data encrypt)"; expect "$out" "encryption at rest is on" "encrypt seals leftovers"
out="$(cb data status)"; refute "$out" "still plaintext" "every file encrypted"
for t in "$CANARY_PERSON" "$CANARY_EMPLOYER" PROJECTZEBRA; do if on_disk "$t"; then fail "plaintext term on disk after encrypt"; fi; done
out="$(cb work gaps 2>&1)"; refute "$out" "cannot decrypt" "careerbrew still reads its data"
event_seen data.encrypt
out="$(CAREERBREW_KEY_FILE="$SCRATCH/missing.key" cb work gaps 2>&1 || true)"
expect "$out" "is missing but data files are encrypted" "a lost key is never silently replaced"
"$ROOT/.venv/bin/python" -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())" > "$SCRATCH/wrong.key"
out="$(CAREERBREW_KEY_FILE="$SCRATCH/wrong.key" cb work gaps 2>&1 || true)"
expect "$out" "cannot decrypt" "a wrong key is a clear error"
# data edit: a terminal opens $EDITOR on a plaintext copy; the file stays encrypted; a pipe is refused
printf '#!/bin/sh\nprintf "edited_in_editor: true\\n" >> "$1"\n' > "$SCRATCH/editor"; chmod +x "$SCRATCH/editor"
expect "$(cb data edit profile.yaml </dev/null 2>&1 || true)" "needs a terminal" "error case: no editor without a terminal"
if [ "$(uname)" = Darwin ]; then
  out="$(EDITOR="$SCRATCH/editor" script -q /dev/null "$ROOT/.venv/bin/careerbrew" data edit profile.yaml </dev/null)"
else
  out="$(EDITOR="$SCRATCH/editor" script -qec "'$ROOT/.venv/bin/careerbrew' data edit profile.yaml" /dev/null </dev/null)"
fi
expect "$out" "saved profile.yaml (encrypted)" "the edit is saved encrypted"
expect "$(data_cat profile.yaml)" "edited_in_editor: true" "the editor's change is kept"
if on_disk edited_in_editor; then fail "the edited file is plaintext on disk"; fi
[ -z "$(ls -A "$CAREERBREW_HOME/.edit" 2>/dev/null)" ] || fail "the plaintext copy was left behind"
# data decrypt: asks for --yes first, then turns encryption off
expect "$(cb data decrypt 2>&1 || true)" "rerun with --yes" "error case: decrypt asks to confirm"
expect "$(cb data decrypt --yes)" "encryption at rest is off" "decrypt writes plaintext"
on_disk PROJECTZEBRA || fail "decrypt left a file encrypted"
echo "ok US-43 encryption at rest"
