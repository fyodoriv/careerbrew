"""Issue #180: teammates in the work feed. A roster, teammate events never noise, and every link in a kept event researched."""

import json

import pytest

from careerbrew import store
from careerbrew.cli import main
from careerbrew.work import feed, team

NOISE = "rules:\n  - {name: chatter, when: {source: slack, kind: message}, triage: noise}\n"


@pytest.fixture
def roster(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T16:00")
    team.add("Sam Lee", chat="U0SAM", login="samlee", email="Sam@Example.com")
    store.write_text(feed.rules_path(), NOISE)
    return home


def _event(i, **kw):
    return {"id": f"slack:C1:{i}", "source": "slack", "kind": "message", "at": "2026-10-21T14:00:00Z", "title": f"message {i}", "actor": "other", **kw}


def _add(*items):
    feed.add_lines("\n".join(json.dumps(i) for i in items))
    return {r["id"]: r for r in feed.load()}


def test_a_teammates_event_is_fyi_not_noise_and_names_the_teammate(roster):
    rows = _add(
        _event(1, actor_id="U0SAM", **{"from": "sam"}, text="shipping the export today"),
        _event(2, actor_id="U0ANA", text="ping <@U0SAM> about the spec"),
        _event(3, text="@samlee can you review?"),
        _event(4, text="lunch?"),
    )
    assert [(rows[f"slack:C1:{i}"]["triage"], rows[f"slack:C1:{i}"]["rule"], rows[f"slack:C1:{i}"]["team"]) for i in range(1, 5)] == [
        ("fyi", "teammate", ["Sam Lee"]),
        ("fyi", "teammate", ["Sam Lee"]),
        ("fyi", "teammate", ["Sam Lee"]),
        ("noise", "chatter", []),
    ]


def test_an_owner_rule_that_names_team_wins_and_team_is_a_rule_key(roster):
    store.write_text(feed.rules_path(), "rules:\n  - {name: mute-sam, when: {team: true, channel: random}, triage: noise}\n" + NOISE.split("\n", 1)[1])
    rows = _add(_event(1, actor_id="U0SAM", channel="random", text="cat pics"), _event(2, actor_id="U0SAM", channel="eng", text="design doc"))
    assert (rows["slack:C1:1"]["triage"], rows["slack:C1:1"]["rule"]) == ("noise", "mute-sam")
    assert (rows["slack:C1:2"]["triage"], rows["slack:C1:2"]["rule"]) == ("fyi", "teammate")
    with pytest.raises(feed.FeedError, match="team is true or false"):
        store.write_text(feed.rules_path(), "rules:\n  - {name: bad, when: {team: yes-please}, triage: fyi}\n")
        feed.rules()


def test_a_name_matches_only_as_a_whole_word(roster):
    rows = _add(_event(1, text="Sam Leeds called"), _event(2, text="ask Sam Lee"), _event(3, text="mail sam@example.com"))
    assert [rows[f"slack:C1:{i}"]["team"] for i in (1, 2)] == [[], ["Sam Lee"]]
    assert rows["slack:C1:3"]["team"] == [], "an email in the text is not the sender; only actor_id or from match an email"


def test_adding_a_teammate_retriages_the_kept_events(roster, capsys):
    _add(_event(1, actor_id="U0ANA", text="the plan is in the doc"))
    assert feed.load()[0]["triage"] == "noise"
    assert main(["work", "team", "add", "Ana Ruiz", "--chat", "U0ANA"]) == 0
    assert "1 changed" in capsys.readouterr().out
    assert (feed.load()[0]["triage"], feed.load()[0]["team"]) == ("fyi", ["Ana Ruiz"])
    assert main(["work", "team", "remove", "ana ruiz"]) == 0
    assert feed.load()[0]["triage"] == "noise"


def test_links_are_found_in_slack_and_bare_form(roster):
    text = "spec <https://docs.example.com/d/1|the spec> and https://tickets.example.com/PROJ-7, see <#C1|eng> <@U0SAM> (https://ci.example.com/b/9)."
    rows = _add(_event(1, actor_id="U0SAM", text=text))
    assert rows["slack:C1:1"]["links"] == ["https://docs.example.com/d/1", "https://tickets.example.com/PROJ-7", "https://ci.example.com/b/9"]


def test_research_is_refused_unless_it_covers_every_link(roster):
    _add(_event(1, actor_id="U0SAM", text="<https://docs.example.com/d/1|spec> and https://sso.example.com/app"))
    row = feed.load()[0]
    assert feed.needs_research(row) == row["links"]
    with pytest.raises(feed.FeedError, match=r"refused: 1 link\(s\) not researched \(link 2 of 2"):
        feed.research(row["id"], [{"url": "https://docs.example.com/d/1", "summary": "the export spec, signed off"}])
    with pytest.raises(feed.FeedError, match="needs a reason"):
        feed.research(row["id"], [{"url": "https://docs.example.com/d/1", "summary": "x"}, {"url": "https://sso.example.com/app", "opened": False}])
    with pytest.raises(feed.FeedError, match="must be one of this event's links"):
        feed.research(row["id"], [{"url": "https://elsewhere.example.com", "summary": "x"}])
    feed.research(row["id"], [{"url": "https://docs.example.com/d/1", "summary": "the export spec, signed off"}, {"url": "https://sso.example.com/app", "opened": False, "reason": "sign-in page"}])
    row = feed.load()[0]
    assert feed.needs_research(row) == [] and [e["opened"] for e in row["research"]] == [True, False]
    feed.retriage()
    assert feed.load()[0]["research"] == row["research"], "research survives a retriage"


def test_noise_and_personal_events_need_no_research(roster):
    rows = _add(_event(1, text="lunch at https://food.example.com"), _event(2, actor_id="U0SAM", text="https://x.example.com", personal=True))
    assert [feed.needs_research(rows[f"slack:C1:{i}"]) for i in (1, 2)] == [[], []]


def test_an_agent_sees_no_teammate_names_or_links(roster, capsys):
    _add(_event(1, actor_id="U0SAM", text="see https://docs.example.com/secret-canary"))
    assert main(["work", "team", "list"]) == 0
    assert main(["work", "feed", "links", "--pending"]) == 0
    out = capsys.readouterr().out
    assert "Sam Lee" not in out and "secret-canary" not in out and "U0SAM" not in out
    assert "1 link(s) not researched yet" in out


def test_the_roster_refuses_a_bad_entry(roster):
    with pytest.raises(team.TeamError, match="needs a name"):
        team.add("  ")
    with pytest.raises(team.TeamError, match="not an email address"):
        team.add("Ana", email="ana")
    with pytest.raises(team.TeamError, match=r"that name is not on your team \(1 teammate"):
        team.remove("nobody")
    team.remove("  sam   LEE ")
    assert team.load() == []
    team.add("Sam Lee", chat="U0SAM")
    assert team.add("sam lee", email="sam2@example.com")["chat"] == "U0SAM", "the same name fills in ids, it never duplicates"
    assert len(team.load()) == 1


def test_errors_never_name_a_teammate_or_a_link(roster):
    """An error reaches an agent's stdout unfiltered (cli.main prints it), so it names counts and positions only."""
    _add(_event(1, actor_id="U0SAM", text="see https://docs.example.com/d?token=canary and https://b.example.com"))
    with pytest.raises(team.TeamError) as e1:
        team.remove("nobody")
    with pytest.raises(feed.FeedError) as e2:
        feed.research("slack:C1:1", [{"url": "https://b.example.com", "summary": "x"}])
    assert "Sam Lee" not in str(e1.value) and "canary" not in str(e2.value) and "link 1 of 2" in str(e2.value)


def test_names_match_as_written_and_bots_only_by_id(roster):
    team.add("Max", chat="U0MAX")
    rows = _add(_event(1, text="raise max retries"), _event(2, text="ask Max"), _event(3, actor="bot", text="Max merged a PR"), _event(4, actor="bot", actor_id="U0MAX", text="deploy"))
    assert [rows[f"slack:C1:{i}"]["team"] for i in range(1, 5)] == [[], ["Max"], [], ["Max"]]


def test_slack_escapes_and_edge_punctuation_in_links(roster):
    text = "<https://x.example.com/a?b=1&amp;c=2|doc> https://en.example.org/wiki/Foo_(bar). `https://y.example.com/z` HTTPS://C.example.com/k,https://d.example.com/m"
    rows = _add(_event(1, actor_id="U0SAM", text=text))
    assert rows["slack:C1:1"]["links"] == ["https://x.example.com/a?b=1&c=2", "https://en.example.org/wiki/Foo_(bar)", "https://y.example.com/z", "HTTPS://C.example.com/k", "https://d.example.com/m"]


def test_research_refuses_a_repeated_link_and_a_noise_or_personal_event(roster):
    rows = _add(_event(1, actor_id="U0SAM", text="https://a.example.com"), _event(2, text="https://n.example.com"))
    with pytest.raises(feed.FeedError, match="the same link twice"):
        feed.research("slack:C1:1", [{"url": "https://a.example.com", "summary": "x"}, {"url": "https://a.example.com", "summary": "y"}])
    assert rows["slack:C1:2"]["triage"] == "noise"
    with pytest.raises(feed.FeedError, match="is noise: only kept events are researched"):
        feed.research("slack:C1:2", [{"url": "https://n.example.com", "summary": "x"}])


def test_adding_the_same_teammate_again_changes_nothing(roster):
    assert team.add("Sam Lee", chat="U0SAM")["changed"] is False
    assert team.add("Sam Lee", email="sam2@example.com")["changed"] is True
