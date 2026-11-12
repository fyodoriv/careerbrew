"""The emitted session brief must require delivery beyond a plan (#333)."""

import pytest

from careerbrew.workday import plan


@pytest.mark.parametrize("required", ["full scope", "one at a time, in order", "shipped and verified", "foreseeable approvals", "each scope item"])
def test_day_plan_gives_every_session_the_full_delivery_contract(required):
    assert required in plan.build()["sessions"]
