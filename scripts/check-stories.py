#!/usr/bin/env python3
"""Gate step: every user story is complete, every story has a skill, and every web surface has approved pictures.

Each `## US-NN:` section in docs/user-stories.md must have:
- **Mode** (job search | job helper | platform);
- an **As** ... **I want** ... **so that** ... line;
- **Acceptance criteria** with >= 2 "Given ... when ... then ..." bullets;
- **Metric** with a **Target**;
- **Skill** naming a declared skill folder inside this checkout;
- **Web views**: the web app views it shows (names from web/src/router.ts, or `Sign-in page`), `all`, or `none`;
  `none (<why>)` when the story talks about the web app but shows no screen of its own (US-75);
- a test table row naming an existing `tests/e2e/us-NN-*.sh`.
A story with web views needs a visual test `web/visual/us-NN-*.spec.ts` and, for each picture it names, an approved
picture at desktop and phone width in web/visual/__screenshots__/ (US-75). Every skill folder must be named by at
least one story.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from careerbrew.agent_workflows import WorkflowSourceError, read_workflow_bytes, repository_workflows

ROOT = Path(__file__).resolve().parents[1]
# The pictures this checker requires: drawn on macOS, where the gate runs (docs/web.md, "Visual tests").
PLATFORM = "darwin"
WIDTHS = ("desktop", "phone")
EXTRA_SCREENS = ("Sign-in page",)
# A story that names one of these talks about the web app, so `none` needs its reason.
WEB_WORDS = re.compile(r"\bweb app\b|web/(?:e2e|src|visual)/|\b(?:Dashboard|Teach me|Job hunt|Library)\b|\b(?:Today|Week|Goals|Standup|Notifications|Interviews|Practice|Work|Review|Reports|Actions|Setup) view\b")


def _views(root: Path) -> list[str]:
    router = (root / "web" / "src" / "router.ts").read_text(encoding="utf-8")
    return sorted(set(re.findall(r'label: "([^"]+)"', router))) + list(EXTRA_SCREENS)


def _web(root: Path, sid: str, num: str, sec: str, views: list[str], specs: dict[str, list[Path]]) -> list[str]:
    """The story's **Web views** line, and the visual test and pictures it needs."""
    line = re.search(r"(?m)^- \*\*Web views\*\*:\s*(.+)$", sec)
    mine = specs.get(num, [])
    if not line:
        return [f"{sid}: no **Web views** (the views it shows, `all`, or `none`)"]
    value = line.group(1).strip()
    if value.startswith("none"):
        problems = [f"web/visual/{s.name}: {sid} says **Web views**: none (fix: list its views, or delete the spec and its pictures)" for s in mine]
        reason = re.fullmatch(r"none\s*\((.+)\)\.?", value)
        if not reason and WEB_WORDS.search(sec.replace(line.group(0), "")):
            problems.append(f"{sid}: says **Web views**: none but names the web app or a view; list its views, or say why in `none (...)`")
        return problems
    names = [] if value == "all" else [n.strip() for n in value.rstrip(".").split(",")]
    problems = [f"{sid}: unknown web view {n} (views: {', '.join(views)})" for n in names if n not in views]
    if not mine:
        return [*problems, f"{sid}: web views {value} but no visual test web/visual/us-{num}-*.spec.ts (fix: write one, then run `npm run visual:update` in web/)"]
    for spec in mine:
        folder = root / "web" / "visual" / "__screenshots__" / spec.name
        have = {p.name for p in folder.glob("*.png")} if folder.is_dir() else set()
        named = sorted(set(re.findall(r"""shot\([^;]*?,\s*["']([\w.-]+)["']\s*(?:,\s*\{[^;]*\})?\)\s*;""", spec.read_text(encoding="utf-8"))))
        wanted = [f"{n}-{w}-{PLATFORM}.png" for n in named for w in WIDTHS]
        problems += [f"{sid}: web/visual/{spec.name} has no approved picture {p} (fix: run `npm run visual:update` in web/ and review it)" for p in wanted if p not in have]
        for w in WIDTHS:  # a spec whose picture names are built in a loop (`view-${view}`) still needs both widths
            if not wanted and not any(p.endswith(f"-{w}-{PLATFORM}.png") for p in have):
                problems.append(f"{sid}: web/visual/{spec.name} has no approved picture at {w} width (fix: run `npm run visual:update` in web/ and review it)")
    return problems


def check(root: Path) -> list[str]:
    root = root.resolve()
    text = (root / "docs" / "user-stories.md").read_text(encoding="utf-8")
    sections = re.split(r"(?m)^## (?=US-\d+:)", text)[1:]
    problems: list[str] = []
    import yaml

    goal_ids = set((yaml.safe_load((root / "config" / "goals.yaml").read_text(encoding="utf-8")) or {}).get("metrics", {}))
    views = _views(root)
    visual = root / "web" / "visual"
    specs: dict[str, list[Path]] = {}
    for spec in sorted(visual.glob("us-*.spec.ts")):
        specs.setdefault(spec.name.split("-")[1], []).append(spec)
    used_skills: set[str] = set()
    host_guide = root / "docs/agent-hosts.md"
    mappings = set(re.findall(r"(?m)^\| `([^`]+)` \|", host_guide.read_text() if host_guide.is_file() else ""))
    inventory = repository_workflows(root)
    problems.extend(inventory.problems)
    skill_folders = {str(w.path.parent.relative_to(root)): w.path.parent for w in inventory.entries if w.kind == "skill"}
    aliases = [p.name.removeprefix("careerbrew-") for p in skill_folders.values()]
    for entry in inventory.entries:
        workflow = entry.path
        try:
            text = read_workflow_bytes(workflow, root).decode("utf-8")
        except (OSError, UnicodeError, WorkflowSourceError):
            problems.append("declared workflow body is outside its checkout or unreadable")
            continue
        if "<!-- careerbrew:dual-host-shared-memory -->" not in text:
            problems.append(f"{workflow.relative_to(root)}: no dual-host/shared-memory host contract")
        if not all(route in text for route in ("careerbrew where", "docs/agent-hosts.md")):
            problems.append(f"{workflow.relative_to(root)}: missing runtime host route (careerbrew where and docs/agent-hosts.md)")
        if entry.kind == "skill" and not workflow.parent.name.startswith("careerbrew-"):
            problems.append(f"{workflow.relative_to(root)}: published skill namespace must use careerbrew- folder names")
        body = text.split("---", 2)[-1]
        primitives = set(re.findall(r"\b(?:AskUserQuestion|TodoWrite|Monitor|SendMessage|run_in_background|get_page_text)\b|\b(?:Task|Agent|Skill)(?=\()|mcp__[A-Za-z0-9_]+|~/.claude/", body))
        for primitive in sorted(primitives - mappings):
            problems.append(f"{workflow.relative_to(root)}: unmapped host primitive {primitive}")
        for alias in aliases:
            if re.search(r"(?:load|run|use|follow|hand off to|with)(?: the)?\s+`" + re.escape(alias) + r"`", body, re.I):
                problems.append(f"{workflow.relative_to(root)}: unqualified sibling skill {alias}")
        if entry.kind == "command":
            name = workflow.stem
            if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name) or name.startswith("x-") or len("agentbrew-command-" + name) > 64:
                problems.append(f"{workflow.relative_to(root)}: name is not native-safe; use a short lower-case hyphenated name or extend provenance-aware deployment checks first")
    for sec in sections:
        sid = sec.split(":", 1)[0].strip()
        num = sid.split("-")[1]
        if not re.search(r"(?m)^- \*\*Host contract\*\*: dual-host-shared-memory$", sec):
            problems.append(f"{sid}: no **Host contract**: dual-host-shared-memory")
        mode = re.search(r"\*\*Mode\*\*:\s*(job search|job helper|platform)", sec)
        if not mode:
            problems.append(f"{sid}: no **Mode** (job search | job helper | platform)")
        if not re.search(r"\*\*As\*\*.+\*\*I want\*\*.+\*\*so that\*\*", sec):
            problems.append(f"{sid}: no As / I want / so that line")
        ac = re.search(r"\*\*Acceptance criteria\*\*:(.*?)(?:\n- \*\*|\n\|)", sec, re.S)
        givens = len(re.findall(r"Given .+?, when .+?, then ", ac.group(1), re.I)) if ac else 0
        if givens < 2:
            problems.append(f"{sid}: needs >= 2 'Given ..., when ..., then ...' acceptance criteria (has {givens})")
        if not re.search(r"\*\*Metric\*\*:.+\*\*Target\*\*", sec):
            problems.append(f"{sid}: no **Metric** with **Target**")
        named = re.search(r"\*\*Metric\*\*:\s*`([a-z_]+)` in `careerbrew review`", sec)
        if named and named.group(1) not in goal_ids:
            problems.append(f"{sid}: metric {named.group(1)} is not computed (add it to config/goals.yaml and review.py)")
        skill = re.search(r"\*\*Skill\*\*:\s*`([a-z0-9_./-]+)`", sec)
        if not skill:
            problems.append(f"{sid}: no **Skill** naming a declared skill folder")
        elif Path(skill.group(1)).as_posix() not in skill_folders:
            problems.append(f"{sid}: skill does not exist in declared sources")
        else:
            used_skills.add(Path(skill.group(1)).as_posix())
        problems += _web(root, sid, num, sec, views, specs)
        tests = re.findall(rf"tests/e2e/us-{num}-[a-z0-9-]+\.sh", sec)
        if not tests:
            problems.append(f"{sid}: no e2e test row tests/e2e/us-{num}-*.sh")
        for t in tests:
            if not (root / t).exists():
                problems.append(f"{sid}: {t} does not exist")
    stories = {sec.split(":", 1)[0].strip().split("-")[1] for sec in sections}
    problems += [f"web/visual/{s.name}: no story US-{num} (fix: name the spec after its story)" for num, ss in specs.items() if num not in stories for s in ss]
    shots = visual / "__screenshots__"
    if shots.is_dir():
        problems += [f"web/visual/__screenshots__/{d.name}: pictures of a spec that does not exist (fix: delete the folder)" for d in sorted(shots.iterdir()) if d.is_dir() and not (visual / d.name).exists()]
    for name, folder in skill_folders.items():
        if name not in used_skills:
            problems.append(f"skill {folder.name} is not named by any story")
    return problems


def main() -> int:
    problems = check(ROOT)
    for p in problems:
        print(f"FAIL  {p}")
    text = (ROOT / "docs" / "user-stories.md").read_text(encoding="utf-8")
    sections = re.split(r"(?m)^## (?=US-\d+:)", text)[1:]
    modes: dict[str, int] = {}
    for sec in sections:
        if mode := re.search(r"\*\*Mode\*\*:\s*(job search|job helper|platform)", sec):
            modes[mode.group(1)] = modes.get(mode.group(1), 0) + 1
    web = sum(1 for sec in sections if (m := re.search(r"(?m)^- \*\*Web views\*\*:\s*(\S+)", sec)) and not m.group(1).startswith("none"))
    skills = sum(entry.kind == "skill" for entry in repository_workflows(ROOT).entries)
    print(f"check-stories: {len(sections)} stories ({', '.join(f'{k}={v}' for k, v in sorted(modes.items()))}), {skills} skills, {web} with web views and pictures, {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
