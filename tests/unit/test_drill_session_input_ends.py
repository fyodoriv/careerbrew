"""`drill session` stops cleanly when its input ends (found by the every-action e2e goal, 2026-10-22).

It crashed with an EOFError traceback when the input ended mid-question (Ctrl-D, or a piped answer
list shorter than the rubric); `cards review` already stopped cleanly there.
"""

import io

from careerbrew.cli import main


def test_input_that_ends_mid_question_stops_the_session_without_a_traceback(home, monkeypatch, capsys):
    assert main(["init"]) == 0
    capsys.readouterr()
    monkeypatch.setattr("sys.stdin", io.StringIO("\n"))  # Enter, then nothing for the rubric questions
    assert main(["drill", "session", "--count", "2"]) == 0
    out = capsys.readouterr().out
    assert "session done: 0 item(s)" in out


def test_ctrl_c_stops_with_the_usual_exit_code(home, monkeypatch, capsys):
    assert main(["init"]) == 0

    def interrupt(_prompt=""):
        raise KeyboardInterrupt

    monkeypatch.setattr("builtins.input", interrupt)
    assert main(["drill", "session", "--count", "1"]) == 130
    assert "session stopped: 0 item(s)" in capsys.readouterr().out


def test_a_full_answer_still_grades_the_item(home, monkeypatch, capsys):
    assert main(["init"]) == 0
    monkeypatch.setattr("sys.stdin", io.StringIO("\n" + "y\n" * 20))
    assert main(["drill", "session", "--count", "1"]) == 0
    assert "session done: 1 item(s)" in capsys.readouterr().out
