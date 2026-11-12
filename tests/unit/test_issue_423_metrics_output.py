import stat
from datetime import date

import pytest

from careerbrew import metrics_share


@pytest.fixture
def enabled_metrics(monkeypatch):
    monkeypatch.setattr(metrics_share, "allowed", lambda: (True, "on"))
    calls = []

    def collect(day):
        calls.append(day)
        return {"tool": "careerbrew", "version": "0", "week": "2026-W45", "machine_id": "synthetic", "metrics": {}}

    monkeypatch.setattr(metrics_share, "collect", collect)
    return calls


@pytest.mark.parametrize("encryption", ["on", "off"])
@pytest.mark.parametrize("via_symlink", [False, True])
def test_metrics_export_refuses_protected_destination_before_collect(home, tmp_path, monkeypatch, enabled_metrics, encryption, via_symlink):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", encryption)
    protected = home / "work"
    protected.mkdir(parents=True)
    dest = protected / "metrics"
    if via_symlink:
        link = tmp_path / "linked-work"
        link.symlink_to(protected, target_is_directory=True)
        dest = link / "metrics"
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))

    with pytest.raises(metrics_share.ShareError, match="data root"):
        metrics_share.write(date(2026, 11, 6))

    assert enabled_metrics == []
    assert list(protected.iterdir()) == []


def test_metrics_export_refuses_output_file_symlink_into_protected_data(home, tmp_path, monkeypatch, enabled_metrics):
    protected = home / "work" / "existing.json"
    protected.parent.mkdir(parents=True)
    protected.write_text("synthetic original")
    dest = tmp_path / "external"
    dest.mkdir()
    (dest / "weekly-2026-W45.json").symlink_to(protected)
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))

    with pytest.raises(metrics_share.ShareError, match="data root"):
        metrics_share.write(date(2026, 11, 6))

    assert enabled_metrics == []
    assert protected.read_text() == "synthetic original"


def test_metrics_export_rechecks_parent_after_collection(home, tmp_path, monkeypatch, enabled_metrics):
    protected = home / "work"
    protected.mkdir(parents=True)
    external = tmp_path / "external"
    external.mkdir()
    link = tmp_path / "metrics-link"
    link.symlink_to(external, target_is_directory=True)
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(link))
    collect = metrics_share.collect

    def swap(day):
        payload = collect(day)
        link.unlink()
        link.symlink_to(protected, target_is_directory=True)
        return payload

    monkeypatch.setattr(metrics_share, "collect", swap)
    with pytest.raises(metrics_share.ShareError, match="data root"):
        metrics_share.write(date(2026, 11, 6))
    assert list(protected.iterdir()) == []


def test_metrics_export_rechecks_output_symlink_after_collection(home, tmp_path, monkeypatch, enabled_metrics):
    protected = home / "work" / "existing.json"
    protected.parent.mkdir(parents=True)
    protected.write_text("synthetic original")
    dest = tmp_path / "external"
    dest.mkdir()
    output = dest / "weekly-2026-W45.json"
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))
    collect = metrics_share.collect

    def swap(day):
        payload = collect(day)
        output.symlink_to(protected)
        return payload

    monkeypatch.setattr(metrics_share, "collect", swap)
    with pytest.raises(metrics_share.ShareError, match="data root"):
        metrics_share.write(date(2026, 11, 6))
    assert protected.read_text() == "synthetic original"


def test_metrics_export_replaces_external_hard_link_without_changing_protected_file(home, tmp_path, monkeypatch, enabled_metrics):
    protected = home / "work" / "existing.json"
    protected.parent.mkdir(parents=True)
    protected.write_text("synthetic original")
    dest = tmp_path / "external"
    dest.mkdir()
    output = dest / "weekly-2026-W45.json"
    output.hardlink_to(protected)
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))

    assert metrics_share.write(date(2026, 11, 6)) == output
    assert protected.read_text() == "synthetic original"
    assert output.read_text() != "synthetic original"


def test_metrics_export_failure_keeps_existing_file_and_cleans_temporary(tmp_path, monkeypatch, enabled_metrics):
    dest = tmp_path / "external"
    dest.mkdir()
    output = dest / "weekly-2026-W45.json"
    output.write_text("synthetic original")
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))

    def fail_replace(*_args):
        raise OSError("synthetic failure")

    monkeypatch.setattr(metrics_share.os, "replace", fail_replace)
    with pytest.raises(OSError, match="synthetic failure"):
        metrics_share.write(date(2026, 11, 6))
    assert output.read_text() == "synthetic original"
    assert list(dest.iterdir()) == [output]


@pytest.mark.parametrize("location", ["external", "plugins", "out"])
def test_metrics_export_keeps_plaintext_destinations(home, tmp_path, monkeypatch, enabled_metrics, location):
    dest = (tmp_path if location == "external" else home / location) / "metrics"
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(dest))
    output = dest / "weekly-2026-W45.json"
    assert metrics_share.write(date(2026, 11, 6)) == output
    assert stat.S_IMODE(output.stat().st_mode) == 0o600
    assert len(enabled_metrics) == 1
