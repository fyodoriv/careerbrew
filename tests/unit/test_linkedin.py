import pytest

from careerbrew import plugins, privacy, store
from careerbrew.cli import main
from careerbrew.work import evidence, linkedin

from .conftest import CANARIES


def _seed(home):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text(f"name: {CANARIES[0]}\nemployer: {CANARIES[1]}\n")
    store.append_jsonl(
        evidence.evidence_path(),
        [
            {"id": "git:payments-core:1", "kind": "commit", "repo": "payments-core", "title": "perf: cut p99 latency 40% in payments-core", "at": "2026-09-01", "tags": ["impact"], "origin": "user"},
            {"id": "jira:PROJ-102", "kind": "jira", "title": "PROJ-102: Migrate billing to new queue", "at": "2026-09-15", "tags": ["impact"], "url": "https://jira.example.com/browse/PROJ-102", "origin": "user"},
            {"id": "note:1", "kind": "note", "title": f"Led the {CANARIES[1]} incident review", "impact": "MTTR down from 3 hours to 40 minutes", "at": "2026-09-20", "tags": ["ownership"], "origin": "user"},
            {"id": "slack:1", "kind": "slack", "title": "Recognition in #eng-wins", "at": "2026-09-21", "tags": ["collaboration"], "origin": "user"},
            {"id": "git:x:2", "kind": "commit", "title": "chore: bump deps", "at": "2026-09-22", "tags": [], "origin": "user"},
        ],
    )


def test_block_is_public_safe_impact_first_and_fits_limit(home):
    _seed(home)
    r = linkedin.build(role="Senior Software Engineer")
    text = r["text"]
    assert text.startswith("Senior Software Engineer.")
    for bad in [*CANARIES[:2], "PROJ-102", "https://", "payments-core", "perf:", "#eng-wins"]:
        assert bad not in text, bad
    assert "40%" in text and "40 minutes" in text, "numbers are kept by default"
    bullets = [line for line in text.splitlines() if line.startswith("•")]
    assert bullets[0].endswith(".") and "MTTR" in bullets[0], "results with stated impact come first"
    assert len(text) <= linkedin.LIMIT and r["path"].read_text().strip() == text


def test_policy_can_forbid_public_metrics(home, acme_plugin):
    _seed(home)
    m = acme_plugin / "careerbrew-plugin.yaml"
    m.write_text(m.read_text().replace("  allowed_routes: [local]", "  allowed_routes: [local]\n  public_metrics: false"))
    plugins.add(str(acme_plugin))
    text = linkedin.build()["text"]
    assert "40%" not in text and "significantly" in text


def test_limit_trims_bullets(home):
    home.mkdir(parents=True, exist_ok=True)
    store.append_jsonl(evidence.evidence_path(), [{"id": f"n{i}", "kind": "note", "title": f"Delivered result number {i} " + "for the platform team " * 20, "at": "2026-09-01", "tags": ["impact"], "origin": "user"} for i in range(10)])
    r = linkedin.build(max_bullets=10)
    assert r["chars"] <= linkedin.LIMIT and r["bullets"] < 10


def test_final_leak_check_blocks(home, monkeypatch):
    _seed(home)
    monkeypatch.setattr(linkedin, "_clean", lambda t, *a: t)  # simulate a scrub bug
    with pytest.raises(privacy.PrivacyError, match="blocked"):
        linkedin.build()


def test_cli_show_prints_public_text(home, capsys):
    _seed(home)
    assert main(["work", "linkedin", "--show"]) == 0
    out = capsys.readouterr().out
    assert "never posts" in out and "•" in out and CANARIES[1] not in out


def test_bullets_read_as_past_tense_results(home):
    _seed(home)
    text = linkedin.build()["text"]
    assert "the an " not in text and "Improved cut" not in text
    assert "Cut p99 latency 40% in an internal service." in text
    assert "Migrated billing to new queue." in text


def test_scrub_removes_emails_hosts_refs_paths_and_inword_terms(home):
    _seed(home)
    evidence.note(
        "fix: email bob@corp.example.com; acme/infra#12 (#123) svc.corp.example.com ~/work/svc and /" + "Users/me/x projectzebrav2",
        ["impact"],
        impact="cut pages 50%; see oncall.corp.example.com",
    )
    (home / "sensitive-terms.txt").write_text("PROJECTZEBRA\n")
    text = linkedin.build()["text"]
    for bad in ["@", "corp.example.com", "acme/infra", "#12", "#123", "~/", "/Users/", "projectzebra", "v2", "; ;", " and."]:
        assert bad.lower() not in text.lower(), bad
    # That note was mostly links and ids, so the gutted-bullet rule drops it entirely.


def test_scrub_keeps_ordinary_slashes():
    from careerbrew import privacy

    assert privacy.scrub_public("Built CI/CD for read/write paths", []) == "Built CI/CD for read/write paths"


def test_gutted_bullets_are_skipped(home):
    home.mkdir(parents=True, exist_ok=True)
    evidence.note("see https://a.example.com/x/y/z and 10.0.0.1 and go/dash and @bob", ["impact"])
    evidence.note("Cut build time 30% for the mobile team", ["impact"])
    text = linkedin.build()["text"]
    assert "Cut build time 30%" in text and "see" not in text.lower().split("\n", 2)[-1]
