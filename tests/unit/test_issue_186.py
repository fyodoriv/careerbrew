"""`day/suggestions.jsonl` holds planner rows and plugin rows; each reader takes only its own (issue #186)."""

from careerbrew import calendar, plugins, review
from careerbrew.cli import main
from careerbrew.day import inbox, planner


def test_plugin_suggestions_skip_the_planners_rows(home, acme_plugin, capsys):
    plugins.add(str(acme_plugin))
    planner.log_suggestion({"kind": "task", "task": "t-1"})
    inbox.suggest("acme", "Review the synthetic roadmap doc", "the roadmap doc changed", "Plans time to review it", ref="doc-1")
    (row,) = inbox.plugin_suggestions()
    assert row["plugin"] == "acme" and row["ref"] == "doc-1"
    assert main(["day", "suggestions"]) == 0
    assert "1 plugin suggestion(s)" in capsys.readouterr().out


def test_the_follow_rate_counts_only_the_planners_rows(home, acme_plugin):
    from careerbrew.work import priorities

    plugins.add(str(acme_plugin))
    task = priorities.add("Write the import test", 3, 3, 1, [])
    planner.log_suggestion({"kind": "task", "task": task["id"]})
    inbox.suggest("acme", "Review the synthetic roadmap doc", "the roadmap doc changed", "Plans time to review it", ref="doc-1")
    priorities.done(task["id"], 1)
    rate = review.story_metrics(calendar.now())["suggestion_follow_rate"]
    assert rate == 1.0, "the one planner suggestion was done the same day; a plugin row is not a planner suggestion"
