"""Custom region labels: a label in `regions_ok` that is not built in matches postings by its own name."""

import json

import pytest
import yaml

from careerbrew.jobs import criteria, platforms, sources

from .conftest import FIXTURES


def _criteria(**fields):
    return {**criteria.DEFAULTS, **fields}


def _posting(region):
    return sources.Posting(id=f"x:{region}", company="AcmeCo", title="Senior Engineer", location="", remote=True, url="https://example.com/x", description="", region=region)


def test_the_built_in_labels_are_broad_regions_only():
    assert [label for label, _words in sources.REGIONS] == ["us", "americas", "europe", "asia", "global"]


def test_a_custom_label_matches_its_own_name_as_a_whole_word():
    custom = criteria.custom_regions(_criteria(regions_ok=["Examplia", "global"]))
    assert custom == [("examplia", ("examplia",))], "built-in labels are not custom"
    assert sources.region_of("Remote, EXAMPLIA", custom) == "examplia"
    assert sources.region_of("Remote, Examplian Isles", custom) == "global", "part of a longer word is no match"


def test_region_words_add_words_for_a_custom_label():
    custom = criteria.custom_regions(_criteria(regions_ok=["examplia"], region_words={"examplia": ["Examplian", "Port Sample"]}))
    assert sources.region_of("Remote (Examplian residents)", custom) == "examplia"
    assert sources.region_of("Hybrid, Port Sample", custom) == "examplia"
    assert sources.region_of("Remote, Examplia", custom) == "examplia", "the name still matches"


def test_a_region_words_label_is_custom_even_when_regions_ok_leaves_it_out():
    c = _criteria(regions_ok=["global"], region_words={"examplia": ["examplian"]})
    custom = criteria.custom_regions(c)
    assert sources.region_of("Remote, Examplia", custom) == "examplia"
    assert criteria.reject_reason(_posting("examplia"), c) == "region"


def test_a_custom_label_wins_over_a_built_in_label():
    custom = criteria.custom_regions(_criteria(regions_ok=["americas", "examplia"]))
    text = "Remote, Examplia, North America"
    assert sources.region_of(text) == "americas"
    assert sources.region_of(text, custom) == "examplia"


def test_text_with_no_custom_match_falls_through_to_the_built_in_labels():
    custom = criteria.custom_regions(_criteria(regions_ok=["examplia"]))
    assert sources.region_of("Remote - US", custom) == "us"
    assert sources.region_of("Anywhere in the World", custom) == "global"
    assert sources.region_of("Hybrid, Example City", custom) == ""


def test_no_custom_labels_without_regions_ok():
    assert criteria.custom_regions(_criteria()) == []
    assert criteria.custom_regions(_criteria(regions_ok=["global", "us"])) == []


@pytest.mark.parametrize(
    ("value", "says"),
    [
        (["examplia"], "must be a mapping"),
        ({"examplia": "examplian"}, "must be a list"),
        ({"examplia": ["examplian", ""]}, "non-empty"),
        ({"examplia": ["examplian", 7]}, "non-empty"),
        ({"": ["examplian"]}, "label"),
        ({"americas": ["examplia"]}, "built-in"),
    ],
)
def test_an_invalid_region_words_setting_is_a_clear_error(value, says):
    with pytest.raises(ValueError, match="region_words") as err:
        criteria.custom_regions(_criteria(regions_ok=["examplia"], region_words=value))
    assert says in str(err.value)


def test_a_posting_saved_with_a_label_passes_when_the_user_lists_that_label(home):
    (home / "jobs").mkdir(parents=True, exist_ok=True)
    sources.save([_posting("examplia"), _posting("europe")])
    saved = sources.load()
    kept, rejected = criteria.apply(saved, _criteria(regions_ok=["Examplia", "global"]))
    assert [p.region for p in kept] == ["examplia"] and rejected["region"] == 1
    kept, rejected = criteria.apply(saved, _criteria(regions_ok=["global"]))
    assert kept == [] and rejected["region"] == 2


def test_a_scan_labels_postings_from_every_kind_of_source(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CB_ADZ_ID", "id-x")
    monkeypatch.setenv("CB_ADZ_KEY", "key-x")
    board = tmp_path / "board.json"
    board.write_text(json.dumps({"jobs": [{"id": 1, "title": "Senior Engineer", "location": {"name": "Remote, Examplia"}, "absolute_url": "https://example.com/1", "content": "Python."}]}))
    entries = [
        {"kind": "greenhouse", "board": "sampleco", "company": "SampleCo", "url": f"file://{board}"},
        {"kind": "adzuna", "country": "gb", "what": "software engineer", "where": "Example City", "app_id": "env:CB_ADZ_ID", "app_key": "env:CB_ADZ_KEY"},
    ]
    (home / "jobs").mkdir(parents=True, exist_ok=True)
    (home / "jobs" / "sources.yaml").write_text(yaml.safe_dump({"sources": entries}))
    (home / "jobs" / "criteria.yaml").write_text(yaml.safe_dump({"regions_ok": ["examplia"], "region_words": {"examplia": ["example city"]}}))
    reads = []
    real = criteria.load
    monkeypatch.setattr(criteria, "load", lambda: reads.append(1) or real())
    ps = {p.id: p for p in sources.scan(no_cache=True)}
    assert ps["gh:SampleCo:1"].region == "examplia"
    assert ps["adzuna:4321"].region == "examplia"
    assert len(reads) == 1, "the criteria file is read once per scan"
    assert platforms.fetch(entries[1], no_cache=True)[0].region == "", "no custom label without the criteria"


def test_a_scan_with_an_invalid_region_words_setting_fails_before_it_saves(home, ats_sources):
    (home / "jobs" / "criteria.yaml").write_text(yaml.safe_dump({"regions_ok": ["examplia"], "region_words": ["examplia"]}))
    with pytest.raises(ValueError, match="region_words"):
        sources.scan(no_cache=True)
    assert not sources.postings_path().exists()


def test_a_scalar_regions_ok_is_one_label_as_before():
    assert criteria.custom_regions(_criteria(regions_ok="global")) == []
    assert criteria.custom_regions(_criteria(regions_ok="Examplia")) == [("examplia", ("examplia",))]
    assert criteria.reject_reason(_posting("global"), _criteria(regions_ok="global")) is None
    assert criteria.reject_reason(_posting("europe"), _criteria(regions_ok="global")) == "region"
    assert criteria.reject_reason(_posting("lob"), _criteria(regions_ok="global")) == "region", "a part of the label is no match"


@pytest.mark.parametrize("value", [[False], [None], ["examplia", 7], [""], {"examplia": True}, 7])
def test_a_regions_ok_entry_that_is_not_text_is_a_clear_error(value):
    with pytest.raises(ValueError, match="regions_ok"):
        criteria.custom_regions(_criteria(regions_ok=value))


def test_a_label_is_a_whole_word_next_to_digits_and_letters_of_any_alphabet():
    custom = criteria.custom_regions(_criteria(regions_ok=["examplia"]))
    assert sources.region_of("Hybrid, Examplia2", custom) == ""
    assert sources.region_of("Hybrid, Exampliaé", custom) == ""
    assert sources.region_of("Hybrid, éExamplia", custom) == ""
    assert sources.region_of("Hybrid (Examplia)", custom) == "examplia"


def test_repeated_spaces_do_not_hide_a_label_of_several_words():
    custom = criteria.custom_regions(_criteria(regions_ok=["examplia"], region_words={"examplia": ["Port   Sample"]}))
    assert sources.region_of("Hybrid, Port \t Sample", custom) == "examplia"
    assert sources.region_of("Remote,  North  America") == "americas"


def test_the_preset_line_says_what_a_country_only_location_reads_as(home, capsys):
    from careerbrew.cli import main

    assert main(["init"]) == 0
    capsys.readouterr()
    assert main(["jobs", "preset", "--force"]) == 0
    out = capsys.readouterr().out
    assert "reads as global unless that country is one of your labels in regions_ok" in out
