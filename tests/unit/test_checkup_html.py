"""The checkup ends with a local HTML page: verdict, blockers, next steps, every section."""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_report_html_shows_verdict_blockers_and_next_steps(tmp_path):
    log = tmp_path / "checkup.log"
    log.write_text("\n== Open work\nPR #3 has a red check\n-> finish these first: conflicts, red checks\n\n== Health\n✗ node missing — fix: brew install node\n\n== Gate\nall passed <ok> & done\n\ncheckup: needs work (see above)\n")
    out = tmp_path / "out" / "checkup.html"
    subprocess.run(["bash", str(ROOT / "scripts" / "report-html.sh"), "demo checkup", str(log), str(out)], check=True)
    page = out.read_text()
    assert "<strong>Verdict:</strong> needs work (see above)" in page
    assert "<li>Health</li>" in page
    assert "<li>finish these first: conflicts, red checks</li>" in page
    assert "all passed &lt;ok&gt; &amp; done" in page
    assert "prefers-color-scheme: dark" in page


def test_checkup_writes_the_page_into_the_ignored_metrics_folder():
    script = (ROOT / "scripts" / "checkup.sh").read_text()
    assert 'scripts/report-html.sh "careerbrew checkup"' in script
    assert ".metrics/checkup.html" in script
    assert any(line.strip().strip("/") == ".metrics" for line in (ROOT / ".gitignore").read_text().splitlines())
