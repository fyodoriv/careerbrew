"""Bypass attempts on the public scrub (from independent grader passes)."""

import pytest

from careerbrew import privacy

TERMS = ["PROJECTZEBRA", "AcmeSecretCustomer"]

LEAKY = [
    "key " + "AKIA" + "ABCDEFGHIJKLMNOP" + " leaked",  # built at runtime so gitleaks never sees a key-shaped literal
    "commit 3f9c2ab1d4e5 fixed it",
    "merged PR !42",
    "closed zebra-42",
    "for Project-Zebra team",
    "for Project Zebra team",
    "for project_zebra team",
    "with Acme Secret Customer",
    "with Acme-Secret-Customer",
    "the ＰＲＯＪＥＣＴＺＥＢＲＡ launch",
    "the PROJ\u200bECTZEBRA launch",
    r"ran C:\Users\bob\zebra\run.ps1",
    r"saved to \\fileserver\share\plans",
    "host 10.0.0.12 and 192.168.1.5:8080",
    "ipv6 [::1] and fe80::1",
    "posted in #team-zebra and #incidents",
    "thanks @jsmith and @alice.w",
    "see /browse/X-1 and /pages/123",
    "use go/zebra-dash",
    "server host-01.lan",
    "mail bob at corp dot example dot com",
    "mail bob[at]corp[dot]com",
    "see https://github.example.com/acme/zebra-billing-svc/pull/9",
]
MUST_GO = {
    0: "AKIA",
    1: "3f9c2ab1d4e5",
    2: "!42",
    3: "zebra-42",
    4: "zebra",
    5: "zebra",
    6: "zebra",
    7: "secret",
    8: "secret",
    9: "ＺＥＢＲＡ",
    10: "ECTZEBRA",
    11: "Users",
    12: "fileserver",
    13: "10.0",
    14: "::1",
    15: "#",
    16: "@",
    17: "/browse",
    18: "go/",
    19: ".lan",
    20: "corp",
    21: "corp",
    22: "pull/9",
}


@pytest.mark.parametrize("i", range(len(LEAKY)))
def test_bypass_is_scrubbed(i):
    out = privacy.scrub_public(LEAKY[i], TERMS)
    assert MUST_GO[i].lower() not in out.lower(), out
    assert "  " not in out and not out.endswith((" and", " see", ":"))


@pytest.mark.parametrize("ok", ["Built CI/CD for read/write paths", "Moved to Node.js 20 and v1.2.3", "Ran an A/B test on client/server code", "Cut p99 latency 40% in 3 weeks"])
def test_ordinary_text_survives(ok):
    assert privacy.scrub_public(ok, TERMS) == ok


def test_find_leaks_sees_variants():
    assert privacy.find_leaks("Project-Zebra rollout", TERMS)
    assert privacy.find_leaks("ＰＲＯＪＥＣＴＺＥＢＲＡ", TERMS)
    assert privacy.find_leaks("PROJ\u200bECTZEBRA", TERMS)


TOKENISH = [
    ("old key " + "sk-" + "ant-abc123def456ghi789" + " revoked", "abc123"),
    ("bot " + "xoxb" + "-123-456-abc" + " rotated", "456"),
    ("bucket s3://acme-zebra-bucket/exports", "s3:"),
    ("pushed to ssh://git@host:22/r", "ssh"),
    ("copied gs://b/x", "gs:"),
    ("DM'd U012ABCDEF about it", "U012"),
    ("deployed to k8s-prod-useast1 cluster", "useast1"),
    ("tracked in jira.corp.example.com/browse/ZBR-7 today", "browse"),
    ("closed ZBR-7", "ZBR-7"),
    ("renamed old_service_name", "old_service"),
]


@pytest.mark.parametrize("text,bad", TOKENISH)
def test_identifier_shaped_tokens_are_dropped(text, bad):
    out = privacy.scrub_public(text, TERMS)
    assert bad.lower() not in out.lower(), out


def test_leetspeak_term_matches():
    assert "zebra" not in privacy.scrub_public("the pr0jectz3bra launch", TERMS).lower()


@pytest.mark.parametrize("ok", ["Kept 24/7 uptime with React/Redux in the U.S.", "Moved k8s jobs and cut p99 3x"])
def test_more_ordinary_text_survives(ok):
    assert privacy.scrub_public(ok, TERMS) == ok


def test_suspects_list_internal_looking_words():
    assert privacy.public_suspects("Ran the zebra-oncall rotation with BillingHub and on-call staff") == ["BillingHub", "zebra-oncall"]


@pytest.mark.parametrize(
    "text,bad",
    [
        ("call +1 415 555 0100 now", "555"),
        ("call 415-555-0100 now", "555"),
        ("at 1234 Main St office", "Main St"),
        ("in room 4B Bldg 12 today", "Bldg"),
        ("employee id 556677 noted", "556677"),
    ],
)
def test_personal_data_is_dropped(text, bad):
    assert bad not in privacy.scrub_public(text, TERMS)


def test_tech_words_with_digits_survive():
    assert privacy.scrub_public("Added OAuth2 and IPv6 support", TERMS) == "Added OAuth2 and IPv6 support"
