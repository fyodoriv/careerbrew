"""Corporate-machine readiness (2026-10-19)."""

from pathlib import Path

import pytest

from careerbrew import cli, machine, store
from careerbrew.work import priorities


def _init():
    cli.main(["init"])


def test_corporate_install_has_no_job_hunt_profile_fields(home):
    _init()  # install.sh runs init first, then sets the role
    machine.set_role("corporate")
    text = store.read_text(home / "profile.yaml")
    for field in machine.JOB_HUNT_FIELDS:
        assert field not in text, field
    assert "current_level" in text and "sensitive" in text


def test_init_on_a_corporate_root_writes_the_work_profile(home):
    home.mkdir(parents=True, exist_ok=True)
    machine.set_role("corporate")
    _init()
    assert "target_salary" not in store.read_text(home / "profile.yaml")


def test_corporate_role_refuses_a_real_profile_with_job_hunt_fields(home):
    _init()
    p = home / "profile.yaml"
    store.write_text(p, store.read_text(p).replace("180000", "123456"))
    with pytest.raises(machine.RoleError, match="profile.yaml"):
        machine.set_role("corporate")


def test_a_task_cannot_be_done_twice(home):
    priorities.add("x", 3, 3, 1, [])
    priorities.done("t001", 1)
    with pytest.raises(ValueError, match="already done"):
        priorities.done("t001", 1)


def test_one_outlier_ratio_cannot_wreck_calibration(home):
    from careerbrew.day import planner

    priorities.add("a", 3, 3, 0.05, [])
    priorities.done("t001", 200)
    assert planner.calibration() <= priorities.MAX_RATIO


def test_text_impact_or_nan_urgency_in_old_tasks_cannot_break_ranking(home):
    priorities.save(
        [
            {"id": "t001", "title": "a", "impact": "high", "urgency": float("nan"), "estimate_h": 1, "competencies": [], "status": "open"},
            {"id": "t002", "title": "b", "impact": 3, "urgency": 3, "estimate_h": 1, "competencies": [], "status": "open"},
        ]
    )
    ranked = priorities.rank({})
    assert all(r["score"] == r["score"] for r in ranked), "no NaN scores"


def test_machine_ready_lists_what_is_missing_then_passes_with_a_real_plugin(home, tmp_path):
    import shutil

    from .conftest import FIXTURES

    plugin = tmp_path / "plugin"
    shutil.copytree(FIXTURES / "plugin-acme", plugin)
    import secrets

    canary = [f"QQ{secrets.token_hex(6).upper()}" for _ in range(4)]  # made at runtime, so no tracked file holds them
    (plugin / "sensitive-terms.txt").write_text(f"{canary[0]}\n{canary[1]}\n")

    _init()
    machine.set_role("corporate")
    failing = {r["check"] for r in machine.readiness() if not r["ok"]}
    assert "a company plugin is installed" in failing
    assert all(r["fix"] for r in machine.readiness() if not r["ok"]), "every failure names its fix"
    from careerbrew import plugins

    plugins.add(str(plugin))
    store.write_text(home / "settings.yaml", store.read_text(home / "settings.yaml").replace("generic-swe", "acme-eng").replace("target_level: senior", "target_level: e5"))
    prof = home / "profile.yaml"
    store.write_text(prof, store.read_text(prof).replace("Your Name", canary[2]).replace("Your Employer", canary[3]))
    rows = machine.readiness()
    assert all(r["ok"] for r in rows), [r for r in rows if not r["ok"]]


def test_machine_ready_catches_company_terms_in_the_base_repo(home, tmp_path):

    from careerbrew import plugins

    from .conftest import FIXTURES

    _init()
    machine.set_role("corporate")
    plugins.add(str(FIXTURES / "plugin-acme"))  # its terms appear in tracked fixture files
    terms_row = next(r for r in machine.readiness() if "company terms" in r["check"])
    assert not terms_row["ok"] and "plugin" in terms_row["fix"]


def test_plugin_refresh_reinstalls_edited_local_plugins_without_naming_them(home, tmp_path):
    import shutil

    from careerbrew import plugins

    from .conftest import FIXTURES

    src = tmp_path / "plugin"
    shutil.copytree(FIXTURES / "plugin-acme", src)
    plugins.add(str(src))
    (src / "rules.md").write_text("- edited after install\n")
    refreshed = plugins.refresh()
    assert refreshed == ["acme"]
    installed = plugins.installed()[0]
    assert (Path(installed["root"]) / "rules.md").read_text() == "- edited after install\n"
    assert installed["source"] == "local-folder", "the lock still never prints the folder"


def test_plugin_new_makes_its_ladder_the_default_and_comments_are_not_terms(home, tmp_path):
    from careerbrew import store, wizard

    _init()
    machine.set_role("corporate")
    wizard.new("zzcorp", tmp_path / "p", [], {}, prompt=False)
    settings = store.read_yaml(home / "settings.yaml", {})
    assert settings["ladder"] == "zzcorp-eng", "the promo doc uses the company ladder"
    rows = {r["check"]: r for r in machine.readiness()}
    assert not rows["sensitive terms list the company plus its codenames"]["ok"], "only the company name so far; the comment line is not terms"
    assert rows["the active ladder comes from a plugin"]["ok"]


def _plugin_copy(tmp_path, name):
    import shutil

    from .conftest import FIXTURES

    dest = tmp_path / name
    shutil.copytree(FIXTURES / name, dest)
    return dest


def test_team_plugin_ladder_wins_over_its_company_plugin_default(home, tmp_path):
    import yaml

    from careerbrew import plugins, store

    base = _plugin_copy(tmp_path, "plugin-acme")
    m = yaml.safe_load((base / "careerbrew-plugin.yaml").read_text())
    m["settings_defaults"] = {"ladder": "acme-eng", "target_level": "e4"}
    (base / "careerbrew-plugin.yaml").write_text(yaml.safe_dump(m))
    plugins.add(str(base))
    assert store.read_yaml(home / "settings.yaml", {})["ladder"] == "acme-eng"
    plugins.add(str(_plugin_copy(tmp_path, "plugin-acme-platform")))
    s = store.read_yaml(home / "settings.yaml", {})
    assert (s["ladder"], s["target_level"]) == ("acme-platform-eng", "e5"), "a team plugin narrows its company plugin"


def test_refresh_orders_by_dependency_and_explains_a_moved_folder(home, tmp_path):
    import shutil

    from careerbrew import plugins

    base, team = _plugin_copy(tmp_path, "plugin-acme"), _plugin_copy(tmp_path, "plugin-acme-platform")
    plugins.add(str(base))
    plugins.add(str(team))
    assert plugins.refresh() == ["acme", "acme-platform"]
    shutil.rmtree(team)
    with pytest.raises(plugins.PluginError) as e:
        plugins.refresh()
    assert str(tmp_path) not in str(e.value), "the folder path is never printed"
    assert "moved or deleted" in str(e.value)


def test_refused_corporate_switch_changes_nothing(home):
    _init()
    p = home / "profile.yaml"
    store.write_text(p, store.read_text(p).replace("180000", "123456"))
    with pytest.raises(machine.RoleError):
        machine.set_role("corporate")
    assert (home / "jobs").exists(), "the refusal comes before any deletion"


def test_example_profile_with_trailing_whitespace_still_counts_as_untouched(home):
    _init()
    p = home / "profile.yaml"
    store.write_text(p, store.read_text(p) + "\n\n")
    machine.set_role("corporate")
    assert "target_salary" not in store.read_text(p)


def test_repo_terms_check_ignores_a_common_word_employer(home, tmp_path):
    import secrets

    from careerbrew import plugins

    _init()
    machine.set_role("corporate")
    plugin = _plugin_copy(tmp_path, "plugin-acme")
    (plugin / "sensitive-terms.txt").write_text(f"QQ{secrets.token_hex(6).upper()}\nQQ{secrets.token_hex(6).upper()}\n")
    plugins.add(str(plugin))
    prof = home / "profile.yaml"
    store.write_text(prof, store.read_text(prof).replace("Your Employer", "Test"))  # "test" appears all over the repo
    row = next(r for r in machine.readiness() if "company terms" in r["check"])
    assert row["ok"], row["fix"]


def test_work_plan_survives_a_task_without_estimate(home):
    priorities.save([{"id": "t001", "title": "a", "impact": 3, "urgency": 3, "competencies": [], "status": "open"}])
    assert cli.main(["work", "plan"]) == 0


def test_template_placeholders_are_not_leak_terms(home):
    from careerbrew import privacy

    _init()
    terms = privacy.sensitive_terms()
    assert "Your Name" not in terms and "Your Employer" not in terms
