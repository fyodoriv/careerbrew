"""Issue #25: `data encrypt` leaves installed plugin copies plaintext, so lock hashes and the term scan keep working."""

from careerbrew import machine, plugins, vault
from careerbrew.cli import main


def test_encrypt_then_ready_needs_no_refresh(home, tmp_path, capsys):
    root = tmp_path / "p"
    (root / "ladders").mkdir(parents=True)
    (root / "ladders" / "x.yaml").write_text("id: acme-eng\nlevels: [senior]\ncompetencies:\n  - {id: d, name: D, weight: 1, signals: {}, expect: {senior: 1}}\n")
    import secrets

    # Made at runtime: a fixed term in this tracked file would trip the repo term scan below.
    (root / "terms.txt").write_text(f"Zq{secrets.token_hex(4)}\nZq{secrets.token_hex(4)}\n")
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\nprovides: {ladders: ladders/*.yaml}\nsensitive_terms: terms.txt\n")
    machine.set_role("corporate", "")
    assert main(["plugin", "add", str(root)]) == 0
    assert main(["data", "encrypt"]) == 0
    assert all(ok for _, ok in plugins.verify()), "lock hashes still match after data encrypt"
    assert not (home / "plugins" / "acme" / "terms.txt").read_bytes().startswith(vault.MAGIC)
    rows = {r["check"]: r["ok"] for r in machine.readiness()}
    assert all(ok for check, ok in rows.items() if "lock hash" in check or "company terms" in check), rows
    capsys.readouterr()
    main(["data", "status"])
    assert "still plaintext" not in capsys.readouterr().out
