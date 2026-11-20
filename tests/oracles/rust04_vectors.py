"""rust-04 vector generator: the one source of the privacy, policy and approval vectors.

Usage: python -I rust04_vectors.py <new-vectors-file>

It writes one JSON file, {"schema": ..., "terms": [...], "sets": {<set>: [<case>, ...]}}, that
both drivers read (the Python oracle `rust04.py` and, later, the Rust
`policy-vectors` binary). It refuses an existing file. Every value is
synthetic and fixed: no clock, no randomness, no data folder, no company or
person names. The canary terms are made-up words; no output may hold them
(set `assert-no-echo`).

docs/rust-migration.md, rust-04 "Vector contract", owns the set list.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

SCHEMA = "careerbrew-rust04-vectors-v1"

# Synthetic canaries: invented words only. Lengths cover both term_pattern
# branches (5+ chars: separators and leetspeak; under 5: exact text).
CANARY_LONG = "Vantrelux"
CANARY_PHRASE = "Zorblat Quenix"
CANARY_SHORT = "Kyx"
CANARY_MID = "Plumbo"
TERMS = [CANARY_PHRASE, CANARY_LONG, CANARY_MID, CANARY_SHORT]

NOW = "2026-01-05T09:00:00+00:00"
LATER = "2026-01-05T10:30:00+00:00"


def _case(cid: str, op: str, **data: Any) -> dict[str, Any]:
    return {"id": cid, "op": op, **data}


def _canary_core() -> list[dict[str, Any]]:
    out = [_case("sensitivities", "sensitivities")]
    out += [_case(f"term-id-{i}", "term-id", term=t) for i, t in enumerate(TERMS)]
    out.append(_case("term-id-case", "term-id", term=CANARY_LONG.upper()))
    texts = {
        "plain": "a plain line",
        "fullwidth": "\uff21\uff22\uff23 wide letters",
        "zero-width": "zero\u200bwidth\u2060join\ufeffmark",
        "ligature": "\ufb01le name",
        "superscript": "x\u00b2 and \u2460",
        "nbsp": "a\u00a0b\u2003c",
        "empty": "",
    }
    out += [_case(f"normalize-{k}", "normalize", text=v) for k, v in texts.items()]
    out += [_case(f"pattern-{i}", "term-pattern", term=t) for i, t in enumerate(TERMS)]
    out.append(_case("pattern-spaced", "term-pattern", term="  " + CANARY_LONG + "  "))
    return out


def _canary_match() -> list[dict[str, Any]]:
    texts = {
        "none": "nothing to see in this note",
        "whole": f"we shipped {CANARY_LONG} today",
        "lower": f"we shipped {CANARY_LONG.lower()} today",
        "upper": f"WE SHIPPED {CANARY_LONG.upper()}",
        "leet": "we shipped V4n7r3lux today",
        "separators": "we shipped V-a_n.t r-e-l-u-x today",
        "zero-width": "we shipped Van\u200btre\u200dlux today",
        "fullwidth": "we shipped \uff36\uff41\uff4e\uff54\uff52\uff45\uff4c\uff55\uff58",
        "inside-word": f"pre{CANARY_LONG}ed",
        "phrase": f"the {CANARY_PHRASE} plan",
        "phrase-joined": "the ZorblatQuenix plan",
        "short-exact": f"ask {CANARY_SHORT} now",
        "short-leet": "ask Ky× now",
        "short-inside": f"a{CANARY_SHORT}b",
        "mid-near-miss": "a Plumb only",
        "multi": f"{CANARY_MID} and {CANARY_LONG} and {CANARY_SHORT}",
        "multiline": "V\na\nn\nt\nr\ne\nl\nu\nx",
    }
    out = [_case(f"find-{k}", "find-leaks", text=v, terms=TERMS) for k, v in texts.items()]
    out.append(_case("find-no-terms", "find-leaks", text=texts["whole"], terms=[]))
    out.append(_case("find-order", "find-leaks", text=texts["multi"], terms=list(reversed(TERMS))))
    return out


def _leak_spans() -> list[dict[str, Any]]:
    texts = {
        "whole": f"line one\nwe shipped {CANARY_LONG}\n",
        "look-alike": "line one\nwe shipped V4ntrelux",
        "inside": f"x\ny\nsub{CANARY_MID}s",
        "words": "the Plu mbo case",
        "lines": "P\nl\nu\nm\nb\no",
        "repeat": f"{CANARY_SHORT} {CANARY_SHORT}\n{CANARY_SHORT}",
        "phrase": f"see {CANARY_PHRASE}\nand {CANARY_PHRASE.lower()}",
        "clean": "nothing here",
        # tests/unit/test_privacy_scan_file.py: the line and how it matched.
        "unit-whole-word": f"## Problem\nthe {CANARY_LONG} repo failed\n",
        "unit-spread-words": "one line\n" + " ".join(CANARY_LONG) + "\n",
        "unit-line-break": f"a\n{CANARY_LONG[:4]}\n{CANARY_LONG[4:]}\n",
        "unit-show-line": f"x\ny\nthe {CANARY_LONG} repo\n",
    }
    return [_case(f"spans-{k}", "leak-spans", text=v, terms=TERMS) for k, v in texts.items()]


def _assert_redact() -> list[dict[str, Any]]:
    texts = {
        "clean": "a clean status line",
        "one": f"status for {CANARY_LONG} is green",
        "two": f"{CANARY_MID} blocks {CANARY_PHRASE}",
        "leet": "status for V4ntr3lux",
        "fullwidth-clean": "\uff21\uff22\uff23 stays wide when clean",
        "fullwidth-dirty": f"\uff21\uff22\uff23 and {CANARY_SHORT}",
        "zero-width-dirty": "Plu\u200bmbo again",
        # tests/unit/test_plugins_privacy.py::test_redact_and_assert_clean:
        # a term and a lowercased second term; redact removes both and the
        # refusal names ids only.
        "unit-hello": f"Hello {CANARY_LONG} at {CANARY_PHRASE.lower()}",
        "empty": "",
        "repeat": f"{CANARY_SHORT}{CANARY_SHORT} {CANARY_SHORT}",
        "multiline": f"one\n{CANARY_MID}\nthree {CANARY_LONG}",
    }
    out = []
    for k, v in texts.items():
        out.append(_case(f"assert-{k}", "assert-clean", text=v, where="stdout", terms=TERMS))
        out.append(_case(f"redact-{k}", "redact", text=v, terms=TERMS))
    # With no terms nothing is redacted, so these inputs hold no canary.
    out.append(_case("redact-no-terms", "redact", text=texts["fullwidth-clean"], terms=[]))
    out.append(_case("assert-no-terms", "assert-clean", text=texts["clean"], where="log", terms=[]))
    out.append(_case("assert-unit-where", "assert-clean", text=texts["unit-hello"], where="test", terms=TERMS))
    # Term order matters: redaction runs term by term on the replaced text.
    out.append(_case("redact-reversed", "redact", text=texts["two"], terms=list(reversed(TERMS))))
    return out


def _allowlists() -> list[dict[str, Any]]:
    out = [_case("constants", "constants")]
    terms_file = "\n".join(["# a comment", CANARY_LONG, "  ", "ab", f"  {CANARY_MID}  ", "#also", CANARY_LONG, CANARY_SHORT])
    out.append(_case("filter-file-only", "filter-terms", file=terms_file, profile={}, plugin_files=[]))
    out.append(
        _case(
            "filter-profile",
            "filter-terms",
            file="",
            profile={"name": "Your Name", "employer": "Your Employer", "email": "zq@x", "sensitive": [CANARY_PHRASE, "no", 7]},
            plugin_files=[],
        )
    )
    out.append(
        _case(
            "filter-profile-real",
            "filter-terms",
            file=CANARY_SHORT,
            profile={"name": CANARY_PHRASE, "employer": CANARY_MID, "email": 42},
            plugin_files=[f"{CANARY_LONG}\n# x\n{CANARY_MID}"],
        )
    )
    out.append(_case("filter-empty", "filter-terms", file="", profile={}, plugin_files=[]))
    tokens = [
        "plain",
        "OAuth2",
        "IPv6",
        "x86_64",
        "UTF-8",
        "v1.2.3",
        "1.20",
        "abc123def",
        "abc12",
        "U012ABCDEF",
        "k8s-prod-east1",
        "and/or",
        "3/4",
        "a/b/c/d",
        "src/main.rs",
        "~/notes",
        "/etc/hosts",
        "x://y",
        "user@host",
        "a\\b",
        "snake_case",
        "(word).",
        "...",
        "",
        "Python3",
        "Python31",
    ]
    out += [_case(f"ident-{i}", "identifier-shaped", token=t) for i, t in enumerate(tokens)]
    texts = {
        "compounds": "a front-end and back-end and follow-up plan",
        "odd-compound": "the river-stone tracker and al-pha",
        "camel": "we use GitHub, TypeScript and FooBar daily",
        "lower-camel": "call parseItem() then saveItem.",
        "mixed": "self-serve LinkedIn quarzo-milano iOS macOS",
        "none": "nothing odd here",
        # tests/unit/test_public_scrub.py::test_suspects_list_internal_looking_words
        "unit-suspects": "Ran the zebra-oncall rotation with BillingHub and on-call staff",
    }
    out += [_case(f"suspects-{k}", "public-suspects", text=v) for k, v in texts.items()]
    return out


def _public_scrub() -> list[dict[str, Any]]:
    texts = {
        "url": "see https://docs.example.com/a/b for details",
        "email": "mail ops@example.org about it",
        "obfuscated-email": "write to ops [at] example [dot] org today",
        "repo-ref": "fixed in some-owner/some-repo#45 and #12",
        "mr": "merged !42 yesterday",
        "mention": "thanks @alex.k for the review",
        "token": "key " + "AKIA" + "ABCDEFGHIJKLMNOP" + " leaked",  # built at runtime so gitleaks never sees a key-shaped literal
        "long-token": "token " + "a1" * 20 + " here",
        "sha": "commit 1a2b3c4d5e6f landed",
        "ipv4": "server " + ".".join(["10", "0", "0", "12"]) + ":8080 was down",  # joined at runtime: no address literal in the tree
        "ipv6": "host [" + "fe80" + "::1]:443 ok",  # joined at runtime
        "windows": "saved to C:\\Users\\x\\file.txt",
        "posix": "edited ~/notes/a.md and ./b.txt",
        "go-link": "see go/plan-doc",
        "hostname": "on db1.internal.example.net now",
        "domain": "visit status.dev",
        "ticket": "closed ZBR-7 and zebra-42",
        "phone": "call +1 " + "-".join(["555", "123", "4567"]),  # joined at runtime
        "address": "at 12 Maple Street",
        "office": "room 4B floor 2",
        "employee": "badge id 12345",
        "term": f"we shipped the {CANARY_LONG} launch",
        "term-leet": "we shipped V4ntr3lux",
        "term-phrase": f"an {CANARY_PHRASE} rollout",
        "dangling": "pushed to https://x.example.com and",
        "parens": "done (see https://a.example.com) ok",
        "and-or": "this and or that",
        "identifier": "ran job_runner_7 with abc123def",
        "clean": "Shipped the onboarding flow; latency dropped 20%.",
        "empty": "",
        # tests/unit/test_public_scrub.py bypass attempts, with the canaries
        # standing in for its terms.
        "unit-sha": "commit 3f9c2ab1d4e5 fixed it",
        "unit-mr": "merged PR !42",
        "unit-hyphen-term": "for Plum-bo team",
        "unit-snake-term": "for plum_bo team",
        "unit-spaced-phrase": "with Zorblat-Quenix",
        "unit-fullwidth-term": "the Ｐｌｕｍｂｏ launch",
        "unit-zero-width-term": "the Van​trelux launch",
        "unit-unc": "saved to \\\\fileserver\\share\\plans",
        "unit-ipv6-short": "ipv6 [::1] and fe80::1",
        "unit-channels": "posted in #team-x and #incidents",
        "unit-mentions": "thanks @jsmith and @alice.w",
        "unit-url-paths": "see /browse/X-1 and /pages/123",
        "unit-lan-host": "server host-01.lan",
        "unit-at-dot": "mail ops at corp dot example dot com",
        "unit-bracket-at": "mail ops[at]corp[dot]com",
        "unit-ordinary": "Cut p99 latency by 40% across 3 services and/or teams.",
        "unit-version": "Upgraded to v1.2.3 and Python3 in Q3",
        "unit-tech-words": "Added OAuth2 and IPv6 support",
        "unit-leet-term": "the pl0mb0 launch",
    }
    out = [_case(f"scrub-{k}", "scrub-public", text=v, terms=TERMS) for k, v in texts.items()]
    out.append(_case("scrub-replacement", "scrub-public", text=texts["term"], terms=TERMS, replacement="a project"))
    out.append(_case("scrub-no-terms", "scrub-public", text=texts["clean"], terms=[]))
    # An empty term matches empty at every place: re.sub puts the replacement there too.
    out.append(_case("scrub-empty-term", "scrub-public", text="we shipped it.", terms=[""]))
    return out


def _private_path() -> list[dict[str, Any]]:
    texts = {
        "bare-word": "keep it in private/ please",
        "relative": "open private/work/day.md",
        "quoted": "path 'private/out/promo.md'",
        "system-tmp": "a /private/tmp/x file",
        "system-var": "see /private/var/folders",
        "system-etc": "(/private/etc/hosts)",
        "deep-system": "x/private/tmp/y",
        "other-root": "a /private/data file",
        "dotted": "the .private/x file",
        "suffix": "myprivate/x",
        "start": "/private/tmp",
        "none": "nothing private here",
        "empty": "",
        "only-head": "private/",
        "second-match": "/private/tmp/x and private/y",
        # tests/unit/test_privacy_scan_file.py: each path is flagged ...
        "unit-dot-relative": "see ./private/day/log.md",
        "unit-home-nested": "in ~/apps/careerbrew/private/out/a.md",
        "unit-paren": "(private/profile.yaml)",
        "unit-backtick": "`private/work/x`",
        "unit-placeholder": "<repo>/private/work/evidence.jsonl",
        "unit-env": "${CAREERBREW_HOME}/private/x",
        "unit-absolute": "/private/work/evidence.jsonl",
        "unit-placeholder-tmp": "<repo>/private/tmp/x",
        "unit-env-var": "$HOME/private/var/x",
        "unit-rel-draft": "the error names private/work/evidence.jsonl",
        # ... and these lines are not.
        "unit-word": "careerbrew never reads `private/`.",
        "unit-tmp-log": "The log was at /private/tmp/run.log.",
        "unit-var-start": "/private/var/folders/x",
        "unit-etc-paren": "(/private/etc/hosts)",
        "unit-suffix": "myprivate/x",
        "unit-all-lines": "careerbrew never reads `private/`.\nThe log was at /private/tmp/run.log.\n/private/var/folders/x\n(/private/etc/hosts)\nmyprivate/x\n",
        "unicode-word-before": "\u00e9private/x",
        "unicode-space-before": "a\u2003/private/tmp/x",
    }
    return [_case(f"path-{k}", "names-private-path", text=v) for k, v in texts.items()]


def _record(rid: str, status: str = "pending", **extra: Any) -> dict[str, Any]:
    rec = {"id": rid, "title": f"title {rid}", "detail": "", "by": "agent", "at": NOW, "status": status, "note": ""}
    rec.update(extra)
    return rec


def _approvals() -> list[dict[str, Any]]:
    base = [_record("aa000001"), _record("aa000002", "approved", note="ok next time", decided_at=NOW), _record("aa000003", "rejected", note="ask first", decided_at=LATER)]
    out = [
        _case("request-simple", "request", records=base, title="Deploy the docs", detail="why", by="agent", now=NOW, new_id="bb000001"),
        _case("request-collapse", "request", records=[], title="  Many \t spaces\n here  ", detail="  keep  inner  ", by="agent", now=NOW, new_id="bb000002"),
        _case("request-long", "request", records=[], title="t" * 250, detail="d" * 4100, by="b" * 80, now=NOW, new_id="bb000003"),
        _case("request-empty", "request", records=[], title=" \n\t ", detail="", by="agent", now=NOW, new_id="bb000004"),
        _case("request-nbsp", "request", records=[], title="a\u00a0b\u2003c", detail="", by="agent", now=NOW, new_id="bb000005"),
        _case("decide-approve", "decide", records=base, item="aa000001", decision="approved", note="  fine  ", now=LATER),
        _case("decide-reject", "decide", records=base, item="aa000001", decision="rejected", note="n" * 1200, now=LATER),
        _case("decide-bad-decision", "decide", records=base, item="aa000001", decision="maybe", note="", now=LATER),
        _case("decide-already", "decide", records=base, item="aa000002", decision="rejected", note="", now=LATER),
        _case("decide-unknown", "decide", records=base, item="zz999999", decision="approved", note="", now=LATER),
        _case("pending", "pending", records=base),
        _case("guidance-default", "guidance", records=base),
        _case("guidance-limit", "guidance", records=base, limit=1),
        _case("guidance-zero", "guidance", records=base, limit=0),
        _case("get-found", "get", records=base, item="aa000003"),
        _case("get-unknown", "get", records=base, item="zz999999"),
        # KeyError's message is repr(): non-printable chars escape as str.isprintable says.
        _case("get-unknown-nonprint", "get", records=base, item="zz\x85\u2028\xa0\u200b\U000e0001"),
    ]
    return out


def _receipt_validate() -> list[dict[str, Any]]:
    good = _record("cc000001")
    shapes: dict[str, list[Any]] = {
        "valid": [good, _record("cc000002", "approved", decided_at=NOW)],
        "empty": [],
        "non-dict": ["text"],
        "missing-id": [{k: v for k, v in good.items() if k != "id"}],
        "missing-status": [{k: v for k, v in good.items() if k != "status"}],
        "unknown-status": [_record("cc000003", "cancelled")],
        "duplicate-id": [_record("cc000004", "approved", decided_at=NOW), _record("cc000004")],
    }
    out = [_case(f"validate-{k}", "validate-records", records=v) for k, v in shapes.items()]
    dup = shapes["duplicate-id"]
    out += [
        _case("dup-get", "get", records=dup, item="cc000004"),
        _case("dup-decide", "decide", records=dup, item="cc000004", decision="approved", note="", now=LATER),
        _case("dup-pending", "pending", records=dup),
        _case("missing-status-pending", "pending", records=shapes["missing-status"]),
        _case("missing-id-get", "get", records=shapes["missing-id"], item="cc000001"),
    ]
    return out


def _directory(**overrides: Any) -> dict[str, Any]:
    d: dict[str, Any] = {
        "channels": [
            {"id": "C100", "owns": ["vpn", "vpn group"], "members": ["U1", "U2", "alex"]},
            {"id": "C200", "owns": ["build role", "ci"], "members": ["U3"]},
        ],
        "threads": [{"channel": "C100", "url": "https://chat.example.com/t/1", "text": "Need the VPN group for new hires"}],
    }
    d.update(overrides)
    return d


def _access() -> list[dict[str, Any]]:
    d = _directory()
    texts = {
        "vpn-ask": "I need access to the vpn group",
        "ci-ask": "blocked: missing build role permission for ci",
        "not-ask": "the vpn is fast today",
        "no-owner": "need permission for the printer",
        "denied": "Access denied, cannot open the CI dashboard",
    }
    out = [_case(f"blocker-{k}", "blocker", text=v) for k, v in texts.items()]
    out += [_case(f"owner-{k}", "owner", text=v, directory=d) for k, v in texts.items()]
    out += [_case(f"route-{k}", "route", text=v, directory=d, teammate="") for k, v in texts.items()]
    out += [
        _case("route-member", "route", text=texts["vpn-ask"], directory=d, teammate="alex"),
        _case("route-non-member", "route", text=texts["vpn-ask"], directory=d, teammate="U9"),
        _case("route-tie", "route", text="need access: vpn and ci", directory=d, teammate=""),
        _case("topic-longest", "topic", text="the VPN GROUP role", channel=d["channels"][0]),
        _case("topic-none", "topic", text="nothing", channel=d["channels"][1]),
        _case("existing-hit", "existing", channel=d["channels"][0], topic="vpn group", directory=d),
        _case("existing-miss", "existing", channel=d["channels"][1], topic="ci", directory=d),
    ]
    mention_texts = {
        "slack": "ping <@U1> and <@U9>",
        "at": "ask @alex and @bob.k-2",
        "email-like": "mail x@y.example",
        "angle": "<@lower> stays",
        "none": "no mentions",
    }
    out += [_case(f"mentions-{k}", "mentions", text=v) for k, v in mention_texts.items()]
    out += [
        _case("judge-ok", "judge", draft="need vpn group access, <@U1>", channel="C100", directory=d, new_thread=False),
        _case("judge-unknown-channel", "judge", draft="need vpn", channel="C999", directory=d, new_thread=False),
        _case("judge-wrong-topic", "judge", draft="need ci", channel="C100", directory=d, new_thread=False),
        _case("judge-duplicate", "judge", draft="need vpn group", channel="C100", directory=d, new_thread=True),
        _case("judge-non-member", "judge", draft="need ci, @alex and <@U3>", channel="C200", directory=d, new_thread=False),
    ]
    shapes: dict[str, Any] = {
        "valid": d,
        "non-dict": ["x"],
        "no-channels": {"threads": []},
        "channels-not-list": {"channels": {}},
        "channel-no-id": {"channels": [{"owns": ["vpn"]}]},
        "members-not-list": {"channels": [{"id": "C1", "owns": ["vpn"], "members": "U1"}]},
        "owns-non-string": {"channels": [{"id": "C1", "owns": [3]}]},
        "threads-null": {"channels": [], "threads": None},
        "duplicate-channel": {"channels": [{"id": "C1"}, {"id": "C1"}]},
    }
    out += [_case(f"validate-{k}", "validate-directory", directory=v) for k, v in shapes.items()]
    out.append(_case("route-threads-null", "route", text=texts["vpn-ask"], directory=shapes["threads-null"] | {"channels": d["channels"]}, teammate=""))
    return out


def _assert_no_echo() -> list[dict[str, Any]]:
    # The oracle checks every set's output against TERMS; this set names the
    # sets it covers so the check itself is a vector both drivers report.
    return [_case("no-echo-all", "no-echo", terms=TERMS)]


GENERATORS = {
    "canary-core": _canary_core,
    "canary-match": _canary_match,
    "leak-spans": _leak_spans,
    "assert-redact": _assert_redact,
    "allowlists": _allowlists,
    "public-scrub": _public_scrub,
    "private-path": _private_path,
    "approvals": _approvals,
    "receipt-validate": _receipt_validate,
    "access": _access,
    "assert-no-echo": _assert_no_echo,
}


def vectors() -> dict[str, Any]:
    """Every set's cases, in the registered order."""
    return {"schema": SCHEMA, "terms": TERMS, "sets": {name: make() for name, make in GENERATORS.items()}}


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: rust04_vectors.py <new-vectors-file>", file=sys.stderr)
        return 2
    out = Path(argv[0])
    try:
        with out.open("x", encoding="utf-8") as fh:
            json.dump(vectors(), fh, ensure_ascii=True, sort_keys=True)
    except FileExistsError:
        print("vectors file already exists", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
