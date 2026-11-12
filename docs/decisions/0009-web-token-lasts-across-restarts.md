# 0009: The web token lasts across restarts; a cookie only gives it back

Date: 2026-10-22. Status: accepted. Issue #141. Changes the "per-run token" of [0006-web-app-on-python-core](0006-web-app-on-python-core.md).

## Context

The web app made a new random token on every start. A start happens on each careerbrew update, so every open tab and bookmark showed "missing or wrong token" many times a day, and users read that as a broken app. The token check must stay: any web page in the browser can send requests to `127.0.0.1`, and the token is what keeps other sites from reading private data.

## Decision

- One token, kept in `day/web-token.json` (encrypted at rest, like every data file), is reused by every start. A new one comes only from `careerbrew web rotate-token` (confirm-to-run) or after `web_token_max_days` (default 30). A running server reads the file again when it changes, so a rotation signs tabs out without a restart. The server reads the token from the file, never from its environment.
- Every API route still needs the `X-Careerbrew-Token` header. A custom header makes any cross-origin request a CORS preflight, which the server never answers, so no other page can call the API.
- A tab that sends the header gets a cookie (`POST /api/session`): host-only for 127.0.0.1, HttpOnly, SameSite=Strict, Path=/, Max-Age until the token expires, named per port. The cookie authenticates one thing only: `GET /api/session` returns the token to careerbrew's own pages, so a reload, a new tab, or a bookmark signs in.
- Cookies ignore the port, so a page on another port of 127.0.0.1 is the same site and its requests carry the cookie. `GET /api/session` therefore refuses any request whose `Sec-Fetch-Site` is not `same-origin` (browsers set it; pages cannot forge it), and every response is `Cross-Origin-Resource-Policy: same-origin`.
- The sign-in page's button (`POST /api/session/open`) needs no token, only `Sec-Fetch-Site: same-origin`. It makes the server open the one-time link in the default browser, as `careerbrew open` does; the reply never holds the token.

## Consequences

- A token is valid for up to 30 days instead of one run, so a leaked token is worth more. It never prints to logs, to `careerbrew serve` without a terminal, or to any URL but the one-time open link, and `rotate-token` ends it at once.
- The token sits in the browser's cookie store (HttpOnly) besides the tab's sessionStorage, as before.
- A browser that sends no `Sec-Fetch-Site` (older than 2023) still restores from the cookie: the same-origin policy and CORP keep the answer from other pages. Its sign-in page button does not work; the page names `careerbrew open`.
- Tests: `tests/unit/test_issue_141.py`, `web/e2e/signin.spec.ts`, `tests/e2e/us-71-web-sign-in.sh`.
