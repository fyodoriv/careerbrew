"""Linux pictures travel from a hosted run's log into a checkout (scripts/visual-pictures.sh, docs/web.md)."""

import hashlib
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/visual-pictures.sh"
WORKFLOW = ROOT / ".github/workflows/visual-pictures.yml"
SPEC = "web/visual/__screenshots__/us-24-day-next.spec.ts"
PICTURE = f"{SPEC}/next-step-phone-linux.png"
INVENTORY = "docs/proof/public-readiness-binaries.tsv"
HEADER = "path\tprovenance\tsha256\tgenerator\n"
# Any PNG of the repo stands in for a Linux picture: the script moves bytes, it draws nothing here.
PNG = next((ROOT / "web/visual/__screenshots__").rglob("*-darwin.png")).read_bytes()


def _run(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "VISUAL_PICTURES_ROOT": str(root)}
    return subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True, env=env, check=False)


def _log(tmp_path: Path, pictures: dict[str, bytes]) -> Path:
    """What `gh run view --log` holds after `draw`: each printed line behind the job, the step and a time."""
    runner = tmp_path / "runner"
    for path, data in pictures.items():
        (runner / path).parent.mkdir(parents=True, exist_ok=True)
        (runner / path).write_bytes(data)
    printed = _run(runner, "print", *pictures)
    assert printed.returncode == 0, printed.stderr
    lines = ["draw (ubuntu-24.04)\tDraw\t2026-11-14T10:00:00.0000000Z " + line for line in printed.stdout.splitlines()]
    log = tmp_path / "run.log"
    log.write_text("draw (ubuntu-24.04)\tSet up job\t2026-11-14T09:59:00.0000000Z ok\n" + "\n".join(lines) + "\n")
    return log


def _checkout(tmp_path: Path) -> Path:
    checkout = tmp_path / "checkout"
    (checkout / "docs/proof").mkdir(parents=True)
    (checkout / INVENTORY).write_text(HEADER + f"{PICTURE}\tgenerated\t{'0' * 64}\tweb/visual/us-24-day-next.spec.ts\n" + "web/z.png\tsynthetic\tabc\t-\n")
    return checkout


def test_a_picture_printed_on_the_runner_is_written_byte_for_byte_with_its_inventory_row(tmp_path):
    other = f"{SPEC}/next-step-desktop-linux.png"
    log = _log(tmp_path, {PICTURE: PNG, other: PNG + b"\0"})
    checkout = _checkout(tmp_path)
    done = _run(checkout, "fetch", "--log", str(log))
    assert done.returncode == 0, done.stderr
    assert "wrote 2 picture(s)" in done.stdout
    assert (checkout / PICTURE).read_bytes() == PNG
    assert (checkout / other).read_bytes() == PNG + b"\0"
    rows = (checkout / INVENTORY).read_text().splitlines(keepends=True)
    assert rows[0] == HEADER
    # The stale row is replaced, the new one added, another file's row kept, and the rows stay sorted.
    assert rows[1:] == [
        f"{other}\tgenerated\t{hashlib.sha256(PNG + bytes(1)).hexdigest()}\tweb/visual/us-24-day-next.spec.ts\n",
        f"{PICTURE}\tgenerated\t{hashlib.sha256(PNG).hexdigest()}\tweb/visual/us-24-day-next.spec.ts\n",
        "web/z.png\tsynthetic\tabc\t-\n",
    ]


def test_a_cut_or_changed_log_writes_no_picture(tmp_path):
    other = f"{SPEC}/next-step-desktop-linux.png"
    log = _log(tmp_path, {other: PNG, PICTURE: PNG})
    lines = log.read_text().splitlines()
    cut = next(i for i, line in enumerate(lines) if f"visual-picture-begin {PICTURE}" in line)
    log.write_text("\n".join(lines[: cut + 3]) + "\n")  # the second picture stops after two lines of base64
    checkout = _checkout(tmp_path)
    before = (checkout / INVENTORY).read_text()
    done = _run(checkout, "fetch", "--log", str(log))
    assert done.returncode == 1
    assert "does not have the SHA-256 the run printed" in done.stderr
    # Not even the first, complete picture is written.
    assert not (checkout / "web").exists()
    assert (checkout / INVENTORY).read_text() == before


def test_a_log_cannot_write_outside_the_linux_pictures(tmp_path):
    log = _log(tmp_path, {PICTURE: PNG})
    good = log.read_text()
    checkout = _checkout(tmp_path)
    for path in (
        "web/visual/__screenshots__/../../../scripts/check.sh",
        "/tmp/next-step-phone-linux.png",
        f"{SPEC}/next-step-phone-darwin.png",
        f"{SPEC}/../x.spec.ts/next-step-phone-linux.png",
        ".github/workflows/ci.yml",
    ):
        log.write_text(good.replace(PICTURE, path))
        done = _run(checkout, "fetch", "--log", str(log))
        assert done.returncode == 1, path
        assert "refused path in the log" in done.stderr, path
    assert not (checkout / "web").exists() and not (checkout / ".github").exists()
    log.write_text("draw\tstep\t2026-11-14T10:00:00Z nothing printed\n")
    done = _run(checkout, "fetch", "--log", str(log))
    assert done.returncode == 1 and "the log holds no picture" in done.stderr


def test_the_drawing_workflow_keeps_the_hosted_safety_rules():
    text = WORKFLOW.read_text()
    gate = (ROOT / ".github/workflows/ci.yml").read_text()
    assert "permissions:\n  contents: read\n" in text
    assert "pull_request_target" not in text and "upload-artifact" not in text and "secrets." not in text
    assert "runs-on: ubuntu-24.04\n" in text and "runs-on: macos" not in text
    uses = [line.split("uses:")[1].strip() for line in text.splitlines() if "uses:" in line]
    # Every action is one the gate already pins, at the same commit, and the browser is installed the same way.
    assert uses and all(f"uses: {use}\n" in gate for use in uses), uses
    assert "npx playwright install --with-deps chromium" in text and "npx playwright install --with-deps chromium" in gate
    assert 'scripts/visual-pictures.sh draw "$UPDATE"' in text
