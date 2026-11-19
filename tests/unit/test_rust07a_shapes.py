"""rust-07a: malformed plugin policy and manifest shapes are refused with fixed messages.

Before this step a string policy list split into characters, the string "false"
counted as true, and a list `provides` crashed. Each refusal names the field and
never echoes the input, so a native port can match the bytes.
"""

import secrets

import pytest

from careerbrew import plugins


def _canary() -> str:
    # Made at runtime: the refusal must never echo it.
    return f"zq{secrets.token_hex(4)}"


def _manifest(**extra):
    return {"name": "acme", "version": "0.1.0", **extra}


MALFORMED = [
    ({"policy": {"allowed_hosts": "CANARY"}}, "policy.allowed_hosts must be a list of strings"),
    ({"policy": {"allowed_routes": ["local", 3]}}, "policy.allowed_routes must be a list of strings"),
    ({"policy": {"backup_targets": "CANARY"}}, "policy.backup_targets must be a list of strings"),
    ({"policy": {"agent_may_read_business": "false"}}, "policy.agent_may_read_business must be true or false"),
    ({"policy": {"require_encrypted_backup": "yes-CANARY"}}, "policy.require_encrypted_backup must be true or false"),
    ({"policy": {"export_to_personal": "CANARY"}}, "policy.export_to_personal must be one of ['none', 'aggregates', 'redacted', 'full']"),
    ({"provides": ["CANARY"]}, "provides must be a mapping of resource kind to paths"),
    ({"requires": ">=LETTERS"}, "requires must name a version with ASCII digits, e.g. >=0.1.0"),
    ({"requires": ">=١.٢"}, "requires must name a version with ASCII digits, e.g. >=0.1.0"),
    ({"sensitive_terms": ["CANARY"]}, "sensitive_terms must be a relative file path inside the plugin folder"),
    ({"sensitive_terms": "../CANARY.txt"}, "sensitive_terms must be a relative file path inside the plugin folder"),
    ({"detect": {"env": "CANARY"}}, "detect.env must be a list of strings"),
    ({"detect": {"paths": "CANARY"}}, "detect.paths must be a list of strings"),
    ({"detect": ["CANARY"]}, "detect must be a mapping with env and paths lists"),
]


@pytest.mark.parametrize(("extra", "message"), MALFORMED)
def test_rust07a_malformed_shape_is_refused_with_a_fixed_message(home, tmp_path, extra, message):
    canary = _canary()
    letters = "".join(c for c in canary if c.isalpha()) or "zq"  # a requires value with no digits

    def plant(value):
        if isinstance(value, str):
            return value.replace("CANARY", canary).replace("LETTERS", letters)
        if isinstance(value, list):
            return [plant(v) for v in value]
        if isinstance(value, dict):
            return {k: plant(v) for k, v in value.items()}
        return value

    with pytest.raises(plugins.PluginError) as err:
        plugins.validate_manifest(_manifest(**plant(extra)), tmp_path)
    assert str(err.value) == message
    assert canary not in str(err.value)
    assert letters not in str(err.value)


def test_rust07a_name_with_a_trailing_newline_is_refused(home, tmp_path):
    with pytest.raises(plugins.PluginError, match="plugin name must match"):
        plugins.validate_manifest(_manifest(name="acme\n"), tmp_path)


def test_rust07a_valid_shapes_still_pass(home, tmp_path):
    data = _manifest(
        requires=">=0.1.0",
        sensitive_terms="terms.txt",
        detect={"env": ["ACME_SSO"], "paths": ["~/Library/acme"]},
        policy={
            "allowed_routes": ["local"],
            "allowed_hosts": [],
            "backup_targets": ["~/backup"],
            "agent_may_read_business": False,
            "require_encrypted_backup": True,
            "public_metrics": False,
            "export_to_personal": "aggregates",
        },
    )
    assert plugins.validate_manifest(data, tmp_path) is data
    assert plugins.validate_manifest(_manifest(requires=None, provides=None, policy=None), tmp_path)


@pytest.mark.parametrize("value", [0, 0.1])
def test_an_unquoted_numeric_requires_is_accepted_like_before(tmp_path, value):
    data = _manifest(requires=value)
    assert plugins.validate_manifest(data, tmp_path) is data
