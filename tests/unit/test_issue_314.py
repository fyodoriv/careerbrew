"""Issue #314: an access ask goes to the owning channel; an open thread is linked, never duplicated; the judge checks each draft's channel."""

from careerbrew.work import access

DIR = {
    "channels": [
        {"id": "C1", "owns": ["vpn", "sso group"], "members": ["U1", "U2"]},
        {"id": "C2", "owns": ["github org"], "members": ["U3"]},
    ],
    "threads": [{"channel": "C1", "url": "https://chat.example.com/C1/p1", "text": "Need the sso group for the data team"}],
}
ASK = "Bo is blocked: missing the sso group, permission denied on the dashboard"


def test_an_access_ask_with_an_existing_thread_links_it_instead_of_opening_a_new_one():
    r = access.route(ASK, DIR, teammate="U2")
    assert r["action"] == "link" and r["channel"] == "C1" and r["thread"] == "https://chat.example.com/C1/p1"
    assert r["tag"] == "in thread"
    assert access.judge("Same here for <@U2>: we need the sso group too", "C1", DIR, new_thread=True) == ["duplicates the open thread https://chat.example.com/C1/p1: link it instead"]


def test_an_ask_with_no_open_thread_opens_one_in_the_owning_channel():
    r = access.route("Request: I need access to the github org, permission denied", DIR, teammate="U1")
    assert r["action"] == "open" and r["channel"] == "C2" and r["tag"] == "link from the original thread"
    assert access.judge("Could someone grant the github org to me?", "C2", DIR, new_thread=True) == []


def test_a_draft_that_mentions_a_non_member_fails_the_judge_step():
    fails = access.judge("<@U3> can you approve the vpn request? cc @carol", "C1", DIR)
    assert fails == ["mentions U3, who is not a member of C1", "mentions carol, who is not a member of C1"]


def test_a_draft_in_a_channel_that_does_not_own_the_topic_fails():
    assert access.judge("Please add me to the github org", "C1", DIR) == ["channel C1 does not own the draft's topic"]
    assert access.judge("vpn please", "C9", DIR) == ["channel C9 is not in the directory"]


def test_a_message_that_is_not_an_access_ask_is_left_alone():
    assert access.route("Lunch at noon?", DIR)["action"] == "none"
    assert access.route("Missing permission for the payroll tool", DIR)["action"] == "unknown"
