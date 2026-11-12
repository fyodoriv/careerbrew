"""Outlook sign-in: the OAuth 2.0 device code flow, and a fresh access token before each Graph read (issue #199).

`careerbrew connect outlook` asks Microsoft for a device code; the owner opens the link and types the code.
careerbrew keeps only the refresh token, in the Keychain (`keychain:careerbrew.outlook.refresh`); the
connector entry holds that reference plus `auth: device`, `client_id`, and `tenant`.

Before each read, `bearer(cfg)` trades the refresh token for an access token (about an hour). Microsoft
returns a new refresh token each time; a Keychain reference gets the new one, so the sign-in keeps working
for weeks. When Microsoft refuses the refresh token (expired, revoked, password changed), the read raises
`ReconnectNeeded`, and `work/outlook-auth.json` records that one step is due: "Reconnect Outlook".

The client ID and tenant come from a flag, the `outlook_client_id` / `outlook_tenant` settings, or a company
plugin's `connector_defaults.outlook` (`client_id`, `tenant`), so a company can use its own app registration.
careerbrew ships no client ID of its own.

Lifecycle: `_ACCESS` is module scope (one per process): access tokens by refresh-token reference, with their
expiry on the process's monotonic clock. A long-running watcher reuses one until a minute before it expires;
nothing writes it to disk. A process exit clears it.
"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from typing import Any

from . import events, paths, secrets, store
from .connectors import ConnectorError, _fixture_path, check_host, open_url

LOGIN = "https://login.microsoftonline.com"
SCOPES = "offline_access User.Read Calendars.ReadBasic Mail.ReadBasic"
REFRESH_SERVICE = "careerbrew.outlook.refresh"
DEFAULT_TENANT = "organizations"  # work and school accounts
DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code"
RECONNECT = "Reconnect Outlook: run `careerbrew connect outlook` in your terminal"
EARLY_S = 60  # refresh a minute before the access token expires

_ACCESS: dict[str, tuple[str, float]] = {}


class ReconnectNeeded(ConnectorError):
    """Microsoft refused the stored sign-in; the owner must run `careerbrew connect outlook` again."""


def settings(client_id: str | None = None, tenant: str | None = None) -> tuple[str, str]:
    """(client_id, tenant): a flag wins, then the owner's settings, then a company plugin's connector_defaults."""
    from . import config
    from .connectors import plugin_defaults

    defaults = plugin_defaults("outlook")
    cid = client_id or str(config.get("outlook_client_id") or "") or str(defaults.get("client_id") or "")
    ten = tenant or str(config.get("outlook_tenant") or "") or str(defaults.get("tenant") or "") or DEFAULT_TENANT
    return cid.strip(), ten.strip()


ENDED = ("invalid_grant", "interaction_required")  # the only answers a new sign-in fixes


def http_form(url: str, fields: dict[str, str]) -> dict[str, Any]:
    """POST a form and return its JSON, an OAuth error answer (HTTP 400) included. Fixtures: `<host>/<path>.json`."""
    check_host(url)
    if os.environ.get("CAREERBREW_HTTP_FIXTURES"):
        return dict(json.loads(_fixture_path(url).read_text(encoding="utf-8")))
    req = urllib.request.Request(url, data=urllib.parse.urlencode(fields).encode(), headers={"Accept": "application/json", "Content-Type": "application/x-www-form-urlencoded"})
    try:
        with open_url(req, timeout=30) as resp:
            return _json(resp.read(), resp.status)
    except urllib.error.HTTPError as e:
        if e.code in (400, 401):
            return _json(e.read() or b"{}", e.code)
        raise


def _json(body: bytes, code: int) -> dict[str, Any]:
    """A proxy or captive portal can answer with a page: say so, never a JSON traceback."""
    try:
        return dict(json.loads(body))
    except (ValueError, TypeError):
        raise ConnectorError(f"outlook: Microsoft sign-in answered HTTP {code}, not JSON (fix: check a proxy or captive portal, then try again)") from None


def _url(tenant: str, leaf: str) -> str:
    return f"{LOGIN}/{urllib.parse.quote(tenant, safe='')}/oauth2/v2.0/{leaf}"


def start(client_id: str, tenant: str) -> dict[str, Any]:
    """Ask for a device code: {device_code, user_code, verification_uri, interval, expires_in}."""
    flow = http_form(_url(tenant, "devicecode"), {"client_id": client_id, "scope": SCOPES})
    if not flow.get("device_code") or not flow.get("user_code"):
        raise ConnectorError(f"outlook: Microsoft gave no device code ({flow.get('error', 'no answer')}) (fix: check outlook_client_id and outlook_tenant; the app registration must allow public client flows)")
    return flow


def poll(client_id: str, tenant: str, flow: dict[str, Any], sleep: Callable[[float], None] = time.sleep, clock: Callable[[], float] = time.monotonic) -> dict[str, Any]:
    """Wait for the owner to enter the code; return the token answer, or raise with a plain reason."""
    interval = float(flow.get("interval") or 5)
    deadline = clock() + float(flow.get("expires_in") or 900)
    fields = {"grant_type": DEVICE_GRANT, "client_id": client_id, "device_code": str(flow["device_code"])}
    while True:
        try:
            ans = http_form(_url(tenant, "token"), fields)
        except OSError:  # one network blip while the owner types the code: wait and ask again, up to the deadline
            ans = {"error": "authorization_pending"}
        if ans.get("access_token") and ans.get("refresh_token"):
            return ans
        err = ans.get("error", "")
        if err == "slow_down":
            interval += 5
        elif err != "authorization_pending":
            reason = {"authorization_declined": "you declined the sign-in", "expired_token": "the code expired", "bad_verification_code": "Microsoft did not know the code"}.get(err, f"Microsoft answered {err or 'with no token'}")
            raise ConnectorError(f"outlook: sign-in did not finish: {reason} (fix: run `careerbrew connect outlook` again)")
        if clock() + interval > deadline:
            raise ConnectorError("outlook: sign-in did not finish: the code expired (fix: run `careerbrew connect outlook` again)")
        sleep(interval)


def _state_path() -> Any:
    return paths.sub("work", "outlook-auth.json")


def needs_reconnect() -> bool:
    return bool((store.read_json(_state_path(), {}) or {}).get("reconnect"))


def _set_reconnect(on: bool) -> None:
    path = _state_path()
    if on or path.exists():
        store.write_json_if_changed(path, {"reconnect": on})  # written only on change, so it never moves /api/changes alone


def signed_in(ref: str) -> None:
    """A finished `connect outlook`: forget an old access token and the reconnect step."""
    _ACCESS.pop(ref, None)
    _set_reconnect(False)


def bearer(cfg: dict[str, Any]) -> str:
    """The token for a Graph call: a fresh access token for a device sign-in, else the stored token itself."""
    ref = str(cfg["token"])
    if cfg.get("auth") != "device":
        return secrets.resolve(ref)
    cached = _ACCESS.get(ref)
    if cached and time.monotonic() < cached[1]:
        return cached[0]
    client_id, tenant = str(cfg.get("client_id") or ""), str(cfg.get("tenant") or DEFAULT_TENANT)
    if not client_id:
        _set_reconnect(True)
        raise ReconnectNeeded(f"outlook: the connector has no client ID ({RECONNECT})")
    try:
        ans = http_form(_url(tenant, "token"), {"grant_type": "refresh_token", "client_id": client_id, "refresh_token": secrets.resolve(ref), "scope": SCOPES})
    except OSError as e:  # no network or a 5xx: the sign-in may still be fine, so never ask to reconnect
        raise ConnectorError(f"outlook: could not reach Microsoft sign-in ({type(e).__name__}) (fix: check the network, then try again)") from None
    if not ans.get("access_token"):
        err = str(ans.get("error") or "")
        if err not in ENDED:  # an app or scope refusal: a new sign-in cannot fix it, so name the code
            raise ConnectorError(f"outlook: Microsoft refused the token ({err or 'no token'}) (fix: check outlook_client_id and outlook_tenant; the app registration must allow public client flows)")
        events.emit("connector.reconnect", source_kind="outlook", status="expired")
        _set_reconnect(True)
        raise ReconnectNeeded(f"outlook: the sign-in expired ({RECONNECT})")
    new = str(ans.get("refresh_token") or "")
    if new and secrets.backend_of(ref) == "keychain":
        try:
            secrets.store_keychain(ref[len("keychain:") :], new)  # Microsoft rotates it; keep the newest
        except secrets.SecretError:  # a locked Keychain: the old refresh token still works, so keep this read going
            pass
    _ACCESS[ref] = (str(ans["access_token"]), time.monotonic() + max(0.0, float(ans.get("expires_in") or 3600) - EARLY_S))
    _set_reconnect(False)
    return str(ans["access_token"])
