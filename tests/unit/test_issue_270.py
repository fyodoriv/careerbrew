"""Issue #270 (US-90): a review ask names enough people for the PR's required approvals."""

from __future__ import annotations

from pathlib import Path

import pytest

from careerbrew import store
from careerbrew.work import review_ask

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures"

SHOP = "https://github.com/example/shop/pull/7"
TOOLS = "https://github.com/example/tools/pull/3"


@pytest.fixture
def github(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_T", "synthetic-not-a-token")
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "token": "env:CB_T", "user": "me"}]})
    return home


def test_a_repo_that_needs_two_approvals_gets_a_draft_with_two_named_people(github):
    r = review_ask.build(SHOP)
    assert r["status"] == "ready"
    assert (r["required"], r["source"], r["needed"]) == (2, "branch protection", 2), "the higher of protection (2) and rulesets (1)"
    assert [p["login"] for p in r["people"]] == ["ana-dev", "ben-dev"], "code owners of the changed paths, most files first"
    assert "- @ana-dev (code owner): src/api/orders.py, src/api/cart.py" in r["text"], "each person is told what to review"
    assert "- @ben-dev (code owner): web/checkout.ts" in r["text"]
    assert "@example/backend" in r["text"] and r["groups"] == ["example/backend"], "a group handle is named but not counted"
    assert review_ask.check(r["text"], r["needed"], r["author"])["ok"], "the built draft passes its own check"


def test_a_draft_with_one_person_plus_a_group_handle_is_refused(github):
    one = review_ask.check("Please review @ana-dev and @example/backend", 2)
    assert not one["ok"] and (one["people"], one["groups"]) == (1, 1)
    assert not review_ask.check_pr(SHOP, "@ana-dev @example/backend @example/everyone")["ok"], "groups never count"
    assert not review_ask.check_pr(SHOP, "@author-dev @ana-dev")["ok"], "the PR's author cannot approve it"
    assert review_ask.check_pr(SHOP, "@ana-dev and @ben-dev, please")["ok"]


def test_never_fewer_than_two_people_even_when_no_approval_is_required(github):
    assert not review_ask.check("@ana-dev", 0)["ok"]
    assert not review_ask.check("@ana-dev", 1)["ok"]
    twice = review_ask.check("@Ana-Dev and @ana-dev", 2)
    assert not twice["ok"] and twice["people"] == 1, "one person named twice in another case counts once"
    assert not review_ask.check("@ana-dev @ben-dev", 3)["ok"], "more when the branch requires more"


def test_recent_reviewers_fill_in_and_a_short_draft_says_how_many_are_missing(github):
    r = review_ask.build(TOOLS)
    assert (r["required"], r["source"]) == (0, "rulesets"), "a refused protection read (403) counts as nothing"
    assert r["needed"] == 2 and r["status"] == "short"
    assert [p["login"] for p in r["people"]] == ["dee-dev"], "a recent reviewer of the same folder; the author and bots are left out"
    assert "Missing: 1 more person(s)" in r["text"]
    r = review_ask.build(TOOLS, ["@eli-dev", "example/maintainers"])
    assert r["status"] == "ready" and [p["login"] for p in r["people"]] == ["eli-dev", "dee-dev"], "a named group is never a person"


@pytest.mark.parametrize(
    ("pattern", "path", "hit"),
    [
        ("*", "a/b.py", True),
        ("/src/api/", "src/api/x/y.py", True),
        ("/src/api/", "lib/src/api/x.py", False),
        ("web/", "web/a.ts", True),
        ("*.ts", "web/deep/a.ts", True),
        ("docs/*", "docs/a.md", True),
        ("docs/*", "docs/sub/a.md", False),
        ("**/logs", "a/b/logs/x", True),
        ("apps/", "apps.py", False),
    ],
)
def test_codeowners_patterns_match_like_github(pattern, path, hit):
    rules = review_ask.parse_codeowners(f"{pattern} @someone")
    assert bool(review_ask.owners_of(path, rules)) is hit


def test_the_last_matching_codeowners_rule_wins():
    rules = review_ask.parse_codeowners("* @all-dev\n/src/ @src-dev @org/team owner@example.com\n")
    assert review_ask.owners_of("src/a.py", rules) == ["src-dev", "org/team"], "an email owner cannot be named"
    assert review_ask.owners_of("README.md", rules) == ["all-dev"]


def test_a_bad_link_or_no_connector_is_refused(home):
    with pytest.raises(review_ask.ReviewAskError, match="not a pull request link"):
        review_ask.build("https://github.com/example/shop/issues/7")
    with pytest.raises(review_ask.ReviewAskError, match="careerbrew connect github"):
        review_ask.build(SHOP)


def test_the_cli_prints_counts_to_an_agent_never_names(github, capsys, monkeypatch):
    from careerbrew import cli

    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["work", "review-ask", SHOP]) == 0
    out = capsys.readouterr().out
    assert "review ask: ready; needs 2 approvals (2 required by branch protection" in out and "names 2 person(s)" in out
    assert "ana-dev" not in out and "src/api" not in out and "checkout" not in out
    assert cli.main(["work", "review-ask", TOOLS]) == 1
    assert "1 more person(s) needed" in capsys.readouterr().out


def test_a_link_to_another_host_is_refused_before_any_request(github, monkeypatch, home):
    # The token goes only to the connector's own API host, never to a host the link names.
    sent = []
    monkeypatch.setattr(review_ask.connectors, "http_json", lambda url, *a, **k: sent.append(url))
    with pytest.raises(review_ask.ReviewAskError, match="not the github connector's host"):
        review_ask.build("https://evil.example.com/example/shop/pull/7")
    store.write_yaml(home / "work" / "connectors.yaml", {"connectors": [{"kind": "github", "token": "env:CB_T", "api": "https://github.example.com/api/v3"}]})
    with pytest.raises(review_ask.ReviewAskError, match="not the github connector's host"):
        review_ask.build(SHOP)
    assert sent == [], "no request, so no token, left the machine"


@pytest.mark.parametrize("word", ["here", "channel", "everyone", "all"])
def test_a_chat_wide_mention_is_not_a_reviewer(word):
    assert not review_ask.check(f"@{word} please review @ana-dev", 2)["ok"]
