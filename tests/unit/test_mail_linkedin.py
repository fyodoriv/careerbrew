import io
import zipfile

import pytest
import yaml

from careerbrew import machine, mail
from careerbrew.cli import main
from careerbrew.jobs import linkedin, sources, strategy

from .conftest import FIXTURES


@pytest.fixture
def accounts(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    for k in ("CB_GRAPH", "CB_FM", "CB_PU", "CB_PP"):
        monkeypatch.setenv(k, "test-value")
    home.mkdir(parents=True, exist_ok=True)
    (home / "mail.yaml").write_text(
        yaml.safe_dump(
            {
                "accounts": [
                    {"name": "work", "kind": "graph", "token": "env:CB_GRAPH", "machine": "corporate"},
                    {"name": "fastmail", "kind": "jmap", "token": "env:CB_FM"},
                    {"name": "proton", "kind": "imap", "host": "127.0.0.1", "port": 1143, "user": "env:CB_PU", "password": "env:CB_PP"},
                ]
            }
        )
    )
    return home


class FakeImap:
    """Minimal imaplib.IMAP4 stand-in, like Proton Mail Bridge on localhost."""

    log: list[str] = []

    def __init__(self, host, port):
        FakeImap.log = [f"connect {host}:{port}"]

    def starttls(self, ssl_context=None):
        FakeImap.log.append("starttls")

    def login(self, user, password):
        FakeImap.log.append("login")

    def select(self, mailbox, readonly=False):
        FakeImap.log.append(f"select {mailbox} readonly={readonly}")
        return "OK", [b"1"]

    def search(self, charset, criteria):
        return "OK", [b"1 2"]

    def fetch(self, num, what):
        assert "BODY.PEEK[HEADER.FIELDS" in what, "only headers, never bodies"
        hdr = {
            "1": b"Message-ID: <p1@proton.me>\r\nFrom: Talent <talent@cedarco.example.com>\r\nSubject: =?utf-8?q?Next_steps=3A_technical_screen?=\r\nDate: Fri, 17 Oct 2026 10:00:00 -0400\r\n\r\n",
            "2": b"Message-ID: <p2@proton.me>\r\nFrom: news@example.com\r\nSubject: Weekly digest\r\nDate: Fri, 17 Oct 2026 11:00:00 -0400\r\n\r\n",
        }[num]
        return "OK", [(b"1 (BODY[HEADER] {100}", hdr), b")"]

    def logout(self):
        FakeImap.log.append("logout")


def test_graph_jmap_and_imap_read_metadata_only(accounts):
    g = mail.graph(mail.accounts()[0], 7)
    assert g[0].sender == "recruiter@northco.example.com" and g[0].subject.startswith("Interview invite")
    j = mail.jmap(mail.accounts()[1], 7)
    assert {m.sender for m in j} == {"jobalerts-noreply@linkedin.com", "jobs-noreply@linkedin.com"}
    p = mail.imap(mail.accounts()[2], 7, factory=FakeImap)
    assert p[0].subject == "Next steps: technical screen" and p[0].sender == "talent@cedarco.example.com"
    assert "select INBOX readonly=True" in FakeImap.log and "starttls" in FakeImap.log and FakeImap.log[-1] == "logout"


def test_classify_groups(accounts):
    msgs = mail.graph(mail.accounts()[0], 7) + mail.jmap(mail.accounts()[1], 7) + mail.imap(mail.accounts()[2], 7, factory=FakeImap)
    groups = mail.classify(msgs)
    assert len(groups["linkedin_alert"]) == 1 and len(groups["linkedin_application"]) == 1
    assert {m.sender for m in groups["interview"]} == {"recruiter@northco.example.com", "talent@cedarco.example.com"}


def test_corporate_machine_reads_only_corporate_accounts(accounts):
    machine.set_role("corporate")
    assert [a["name"] for a in mail.accounts()] == ["work"]


def test_alert_subject_becomes_posting(accounts):
    j = mail.jmap(mail.accounts()[1], 7)
    added = linkedin.add_alert_postings(j)
    assert added == 1
    (p,) = [p for p in sources.load() if p.source_name == "LinkedIn alert"]
    assert (p.company, p.title, p.seniority) == ("ZzPlatformCo", "Senior Software Engineer", 3)
    assert linkedin.add_alert_postings(j) == 0, "idempotent by message id"


def test_mail_scan_cli_on_personal_machine(accounts, monkeypatch, capsys):
    monkeypatch.setattr(mail, "READERS", {**mail.READERS, "imap": lambda acc, days: mail.imap(acc, days, factory=FakeImap)})
    assert main(["mail", "scan"]) == 0
    out = capsys.readouterr().out
    assert "LinkedIn alerts: 1 new posting" in out and "2 interview or recruiter email" in out
    assert (accounts / "jobs" / "inbox.jsonl").exists()


def _export(tmp_path):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("Positions.csv", "Company Name,Title,Description,Location,Started On,Finished On\nOldCo,Software Engineer,Built things,Example City,Jan 2020,Dec 2023\n")
        zf.writestr("Skills.csv", "Name\nPython\nKubernetes\nGraphQL\n")
        zf.writestr("Connections.csv", 'Notes:\n"When exporting your connection data..."\n\nFirst Name,Last Name,URL,Email Address,Company,Position,Connected On\nA,B,https://x,,C,D,01 Jan 2024\n')
        zf.writestr("Jobs/Saved Jobs.csv", "Saved Date,Job Url,Job Title,Company Name\n10/1/26,https://www.linkedin.com/jobs/view/1,Senior Software Engineer,BirchCo\n")
        zf.writestr("Jobs/Job Applications_1.csv", "Application Date,Contact Email,Contact Phone Number,Company Name,Job Title,Job Url,Resume Name,Question And Answers\n9/1/26,,,ElmCo,Staff Engineer,https://x,cv.pdf,\n")
    p = tmp_path / "export.zip"
    p.write_bytes(buf.getvalue())
    return p


def test_linkedin_export_import(home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    (home / "profile.yaml").write_text("skills: {python: 4}\n")
    r = linkedin.import_export(_export(tmp_path))
    assert r == {"positions": 1, "skills_suggested": 2, "saved_jobs": 1, "applications_logged": 1}
    assert any(p.company == "BirchCo" and p.source_name == "LinkedIn saved" for p in sources.load())
    assert any(row["company"] == "ElmCo" for row in __import__("careerbrew.store", fromlist=["x"]).read_jsonl(strategy.applications_path()))
    assert linkedin.import_export(_export(tmp_path))["applications_logged"] == 0, "re-import does not duplicate"


def test_scan_keeps_linkedin_postings(home, tmp_path):
    home.mkdir(parents=True, exist_ok=True)
    linkedin.import_export(_export(tmp_path))
    (home / "jobs" / "sources.yaml").write_text("sources: []\n")
    sources.scan()
    assert any(p.source_name == "LinkedIn saved" for p in sources.load())
