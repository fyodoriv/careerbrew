from pathlib import Path

import pytest

from careerbrew import plugins, wizard


@pytest.mark.parametrize("encryption", ["on", "off"])
@pytest.mark.parametrize("via_symlink", [False, True])
def test_plugin_source_refuses_protected_data_before_writing(home, tmp_path, monkeypatch, encryption, via_symlink):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", encryption)
    protected = home / "work"
    protected.mkdir(parents=True)
    parent = protected
    if via_symlink:
        parent = tmp_path / "linked-work"
        parent.symlink_to(protected, target_is_directory=True)
    dest = parent / "plugin-source"

    with pytest.raises(plugins.PluginError, match="data root"):
        wizard.scaffold("Synthetic", dest, [], {})

    assert not dest.exists()
    assert list(protected.iterdir()) == []


@pytest.mark.parametrize("location", ["external", "plugins", "out"])
def test_plugin_source_allows_plaintext_locations(home: Path, tmp_path: Path, location: str):
    dest = (tmp_path if location == "external" else home / location) / "plugin-source"
    assert wizard.scaffold("Synthetic", dest, [], {}) == dest
    assert (dest / "careerbrew-plugin.yaml").is_file()
    assert (dest / ".git" / "HEAD").is_file()
