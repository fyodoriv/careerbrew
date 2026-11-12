"""Credentials by reference only. careerbrew never writes a token to a file.

Config files hold a reference, resolved at call time:
- `keychain:<service>`  macOS Keychain generic password (account = $USER)
- `op://<vault>/<item>/<field>`  1Password CLI (`op read`), common on corporate machines
- `env:<NAME>`          an environment variable (CI, or a secret manager that exports env)

A plugin policy `secret_backends: [keychain, op]` limits which kinds are allowed
(for example, a company that requires 1Password). Resolved values are never
logged, cached, or printed.
"""

from __future__ import annotations

import getpass
import os
import shlex
import subprocess

from . import events, plugins


class SecretError(RuntimeError):
    pass


class MissingSecretError(SecretError):
    """A permitted reference resolved to no value."""


def backend_of(ref: str) -> str:
    if ref.startswith("keychain:"):
        return "keychain"
    if ref.startswith("op://"):
        return "op"
    if ref.startswith("env:"):
        return "env"
    raise SecretError("a secret must be a reference (keychain:<service>, op://vault/item/field, or env:NAME), never a literal value")


def _check_policy(kind: str) -> None:
    allowed = plugins.effective_policy().get("secret_backends")
    if allowed is not None and kind not in allowed:
        raise SecretError(f"secret backend {kind} is not allowed by plugin policy (allowed: {allowed})")


def resolve(ref: str) -> str:
    kind = backend_of(ref)
    _check_policy(kind)
    if kind == "env":
        value = os.environ.get(ref[4:], "")
    elif kind == "keychain":
        res = subprocess.run(["security", "find-generic-password", "-s", ref[9:], "-a", getpass.getuser(), "-w"], capture_output=True, text=True, check=False)
        value = res.stdout.strip() if res.returncode == 0 else ""
    else:
        res = subprocess.run(["op", "read", ref], capture_output=True, text=True, check=False)
        value = res.stdout.strip() if res.returncode == 0 else ""
    if not value:
        events.emit("secret.missing", source_kind=kind)
        raise MissingSecretError(f"secret {kind} reference did not resolve (fix: `careerbrew connect <name>` or check the reference)")
    return value


def store_keychain(service: str, value: str) -> str:
    """Save a token to the macOS Keychain and return its reference."""
    _check_policy("keychain")
    # `security -i` reads the command from stdin, so the token never appears in argv (`ps`).
    line = " ".join(shlex.quote(x) for x in ["add-generic-password", "-U", "-s", service, "-a", getpass.getuser(), "-w", value])
    res = subprocess.run(["security", "-i"], input=line + "\n", capture_output=True, text=True, check=False)
    if res.returncode != 0:
        raise SecretError("could not write to the Keychain (fix: unlock the login keychain)")
    return f"keychain:{service}"
