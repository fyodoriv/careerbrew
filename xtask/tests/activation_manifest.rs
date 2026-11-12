use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const MANIFEST: &str = include_str!("../../docs/plans/2026-11-04-rust-activation-manifest.tsv");
const PROVIDERS: &str = include_str!("../../docs/plans/2026-11-04-rust-activation-providers.tsv");
const WORKDAY: &str = include_str!("../../docs/plans/2026-11-04-rust-activation-workday.tsv");
const OWNERS: &[&str] = &[
    "rust-02", "rust-05", "rust-07", "rust-10", "rust-14", "rust-15", "rust-16", "rust-17",
    "rust-18", "rust-19", "rust-20", "rust-21", "rust-22", "rust-24", "rust-25", "rust-26",
];

#[derive(Debug)]
struct Node<'a> {
    path: &'a str,
    line: usize,
}

#[derive(Debug)]
struct Call<'a> {
    kind: &'a str,
    from: &'a str,
    to: &'a str,
    line: usize,
    expression: &'a str,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DataUse<'a> {
    artifact: &'a str,
    access: &'a str,
    caller: &'a str,
    line: usize,
    expression: &'a str,
}

#[derive(Debug)]
struct Manifest<'a> {
    nodes: BTreeMap<&'a str, Node<'a>>,
    calls: Vec<Call<'a>>,
    unresolved: BTreeSet<(&'a str, usize, &'a str)>,
    data_uses: BTreeSet<DataUse<'a>>,
}

fn line_number(text: &str) -> Result<usize, &'static str> {
    text.parse()
        .ok()
        .filter(|line| *line > 0)
        .ok_or("invalid line")
}

fn module(symbol: &str) -> &str {
    symbol.split_once('.').unwrap().0
}

fn name(symbol: &str) -> &str {
    symbol.rsplit_once('.').unwrap().1
}

fn module_path(path: &str) -> Option<String> {
    let relative = path.strip_prefix("src/careerbrew/")?.strip_suffix(".py")?;
    relative
        .split('/')
        .all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .then(|| relative.replace('/', "."))
}

fn owns_symbol(path: &str, symbol: &str) -> bool {
    let Some(module) = module_path(path) else {
        return false;
    };
    let Some(member) = symbol.strip_prefix(&format!("{module}.")) else {
        return false;
    };
    let method = member.split_once('.');
    match method {
        Some((class, method)) => {
            class.starts_with(|c: char| c.is_ascii_uppercase()) && !method.contains('.')
        }
        None => !member.is_empty(),
    }
}

fn combined_input() -> String {
    let providers = PROVIDERS
        .strip_prefix("activation-manifest-v1\td6b798a\tpartial\n")
        .expect("provider manifest revision and partial coverage");
    format!("{MANIFEST}{providers}")
}

fn parse(text: &str) -> Result<Manifest<'_>, &'static str> {
    let mut rows = text.lines();
    if rows.next() != Some("activation-manifest-v1\te18041d\tpartial") {
        return Err("invalid header");
    }
    let mut nodes = BTreeMap::new();
    let mut calls = Vec::new();
    let mut sites = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    let mut data_uses = BTreeSet::new();
    for row in rows {
        match row.split('\t').collect::<Vec<_>>().as_slice() {
            ["node", symbol, owner, path, line]
                if OWNERS.contains(owner) && symbol.contains('.') =>
            {
                if !owns_symbol(path, symbol) {
                    return Err("invalid node path");
                }
                let node = Node {
                    path,
                    line: line_number(line)?,
                };
                if nodes.insert(*symbol, node).is_some() {
                    return Err("duplicate node");
                }
            }
            [kind @ ("cross" | "local"), from, to, line, expression]
                if nodes.contains_key(from) && nodes.contains_key(to) && !expression.is_empty() =>
            {
                if (*kind == "local") != (nodes[from].path == nodes[to].path) {
                    return Err("incorrect edge kind");
                }
                let line = line_number(line)?;
                if !sites.insert((*from, *to, line)) {
                    return Err("duplicate call site");
                }
                calls.push(Call {
                    kind,
                    from,
                    to,
                    line,
                    expression,
                });
            }
            ["unresolved", from, line, expression, reason]
                if nodes.contains_key(from) && !expression.is_empty() && !reason.is_empty() =>
            {
                if !unresolved.insert((*from, line_number(line)?, *expression)) {
                    return Err("duplicate unresolved boundary");
                }
            }
            ["data", artifact, access @ ("read" | "write"), caller, line, expression]
                if nodes.contains_key(caller) && !artifact.is_empty() && !expression.is_empty() =>
            {
                let use_site = DataUse {
                    artifact,
                    access,
                    caller,
                    line: line_number(line)?,
                    expression,
                };
                if !data_uses.insert(use_site) {
                    return Err("duplicate data use");
                }
            }
            _ => return Err("invalid row"),
        }
    }
    Ok(Manifest {
        nodes,
        calls,
        unresolved,
        data_uses,
    })
}

fn read_source(node: &Node<'_>) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::read_to_string(root.join(node.path)).unwrap()
}

fn check_anchor(manifest: &Manifest<'_>, symbol: &str, line: usize, expression: &str) {
    let node = &manifest.nodes[symbol];
    let source = read_source(node);
    let lines: Vec<_> = source.lines().collect();
    let declaration = lines[node.line - 1];
    assert!(
        declaration
            .trim_start()
            .starts_with(&format!("def {}(", name(symbol))),
        "{symbol} declaration changed"
    );
    let index = line - 1;
    assert!(
        lines[index].contains(expression),
        "{symbol} anchor {line} changed"
    );
    if line < node.line {
        assert_eq!(line + 1, node.line, "{symbol} decorator moved");
        assert!(
            expression.starts_with('@'),
            "{symbol} anchor precedes its declaration"
        );
    } else {
        let indent = declaration.len() - declaration.trim_start().len();
        assert!(
            lines[node.line..=index].iter().all(|line| {
                let trimmed = line.trim_start();
                trimmed.is_empty()
                    || trimmed.starts_with('#')
                    || line.len() - trimmed.len() > indent
            }),
            "{symbol} anchor is outside its function"
        );
    }
}

fn components<'a>(
    manifest: &Manifest<'a>,
    extra: Option<(&'a str, &'a str)>,
) -> Vec<BTreeSet<&'a str>> {
    let mut graph: BTreeMap<_, Vec<_>> = manifest
        .nodes
        .keys()
        .map(|symbol| (*symbol, Vec::new()))
        .collect();
    for (from, to) in manifest
        .calls
        .iter()
        .map(|call| (call.from, call.to))
        .chain(extra)
    {
        graph.get_mut(from).unwrap().push(to);
    }
    let reachable = |start| {
        let mut pending = vec![start];
        let mut seen = BTreeSet::new();
        while let Some(node) = pending.pop() {
            if seen.insert(node) {
                pending.extend(graph[node].iter().copied());
            }
        }
        seen
    };
    let mut unassigned: BTreeSet<_> = graph.keys().copied().collect();
    let mut groups = Vec::new();
    while let Some(&first) = unassigned.first() {
        let group: BTreeSet<_> = reachable(first)
            .into_iter()
            .filter(|node| reachable(node).contains(first))
            .collect();
        unassigned.retain(|node| !group.contains(node));
        groups.push(group);
    }
    groups
}

#[test]
fn verifies_twenty_four_cross_module_calls() {
    let manifest = parse(MANIFEST).unwrap();
    let calls: Vec<_> = manifest
        .calls
        .iter()
        .filter(|call| call.kind == "cross")
        .collect();
    let by_area = |area| {
        calls
            .iter()
            .filter(|call| module(call.from) == area)
            .count()
    };
    assert_eq!(
        [
            calls.len(),
            by_area("config"),
            by_area("vault"),
            by_area("approvals")
        ],
        [24, 4, 3, 7]
    );
    let expected = [
        ("config._file", "store.read_yaml"),
        ("config.set_value", "store.write_yaml"),
        ("config.set_value", "events.emit"),
        ("config.unset", "store.write_yaml"),
        ("vault._fernet", "backup.load_key"),
        ("vault._key", "backup.key_path"),
        ("vault.open_bytes", "backup.key_path"),
        ("approvals.load", "store.read_json"),
        ("approvals.request", "store.write_json"),
        ("approvals.request", "events.emit"),
        ("approvals.request", "calendar.now_utc"),
        ("approvals.decide", "store.write_json"),
        ("approvals.decide", "events.emit"),
        ("approvals.decide", "calendar.now_utc"),
        ("cli.main", "machine.guard"),
        ("cli.may_show_business", "plugins.effective_policy"),
        ("server.Handler._guard", "machine.role"),
        ("machine.info", "store.read_yaml"),
        ("machine.suggest", "plugins.installed"),
        ("plugins._lock", "store.read_json"),
        ("plugins.installed", "store.read_yaml"),
        ("store.read_text", "vault.decode"),
        ("store.write_text", "vault.encode"),
        ("calendar.local_tz", "store.read_yaml"),
    ];
    assert_eq!(
        calls
            .iter()
            .map(|call| (call.from, call.to))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(expected)
    );
    for call in calls {
        assert!(call.expression.starts_with(call.to));
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn verifies_twenty_two_local_bridges() {
    let manifest = parse(MANIFEST).unwrap();
    let calls: Vec<_> = manifest
        .calls
        .iter()
        .filter(|call| call.kind == "local")
        .collect();
    assert_eq!(calls.len(), 22);
    for call in calls {
        assert!(call.expression.contains(&format!("{}(", name(call.to))));
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn requires_unique_owned_nodes_and_valid_edges() {
    let manifest = parse(MANIFEST).unwrap();
    assert_eq!(manifest.nodes.len(), 36);
    let node = MANIFEST
        .lines()
        .find(|row| row.starts_with("node\t"))
        .unwrap();
    let edge = MANIFEST
        .lines()
        .find(|row| row.starts_with("cross\t"))
        .unwrap();
    for duplicate in [node, edge] {
        assert!(parse(&format!("{MANIFEST}{duplicate}\n")).is_err());
    }
    assert!(parse(&MANIFEST.replacen("rust-05", "rust-99", 1)).is_err());
    assert!(parse(&format!(
        "{MANIFEST}cross\tconfig._file\tstore.missing\t259\tstore.missing(\n"
    ))
    .is_err());
}

#[test]
fn keeps_all_unresolved_boundaries_visible() {
    let manifest = parse(MANIFEST).unwrap();
    let expected = BTreeSet::from([
        ("cli.main", "args.fn(args)"),
        (
            "cli.may_show_business",
            "sys.stdout.isatty() or _owner_ui()",
        ),
        ("config.set_value", "DEFAULTS[key][1](raw)"),
        ("vault._fernet", "@lru_cache(maxsize=4)"),
    ]);
    assert_eq!(
        manifest
            .unresolved
            .iter()
            .map(|(from, _, expression)| (*from, *expression))
            .collect::<BTreeSet<_>>(),
        expected
    );
    for (from, line, expression) in &manifest.unresolved {
        check_anchor(&manifest, from, *line, expression);
    }
}

#[test]
fn has_only_singleton_components_in_bounded_graph() {
    let manifest = parse(MANIFEST).unwrap();
    let groups = components(&manifest, None);
    assert_eq!(groups.len(), manifest.nodes.len());
    assert!(groups.iter().all(|group| group.len() == 1));
}

#[test]
fn detects_planted_three_function_cycle() {
    let manifest = parse(MANIFEST).unwrap();
    let groups = components(&manifest, Some(("backup.load_key", "vault._key")));
    let nontrivial: Vec<_> = groups.into_iter().filter(|group| group.len() > 1).collect();
    assert_eq!(
        nontrivial,
        [BTreeSet::from([
            "backup.load_key",
            "vault._fernet",
            "vault._key"
        ])]
    );
}

fn provider_calls<'a, 'm>(manifest: &'m Manifest<'a>) -> &'m [Call<'a>] {
    let prior_count = parse(MANIFEST).unwrap().calls.len();
    &manifest.calls[prior_count..]
}

fn binding<'a>(target: &'a str, source: &str) -> Option<&'a str> {
    let import = match target {
        "work.promo" => "from .work import promo",
        "day.planner" => "from .day import planner",
        "jobs.sources" => "from .jobs import sources, strategy",
        "jobs.strategy"
            if source
                .lines()
                .any(|line| line.trim() == "from .jobs import sources, strategy") =>
        {
            "from .jobs import sources, strategy"
        }
        "jobs.strategy" => "from .jobs import strategy",
        "tutor.engine" => "from ..tutor import engine as tutor",
        "tutor.exercises" => "from .tutor import cards, exercises, mistakes",
        "tutor.mistakes" => "from . import mistakes",
        _ if !target.contains('.') => return Some(target),
        _ => return None,
    };
    if !source.lines().any(|line| line.trim() == import) {
        return None;
    }
    Some(if target == "tutor.engine" {
        "tutor"
    } else {
        name(target)
    })
}

#[test]
fn providers_pin_fifty_cross_module_sites() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let calls: Vec<_> = provider_calls(&manifest)
        .iter()
        .filter(|call| call.kind == "cross")
        .collect();
    let changes = calls
        .iter()
        .filter(|call| call.from.starts_with("changes."))
        .count();
    let teacher = calls
        .iter()
        .filter(|call| {
            call.from.starts_with("teacher.") || call.from.starts_with("tutor.exercises.")
        })
        .count();
    assert_eq!(
        [
            calls.len(),
            changes,
            calls.len() - changes - teacher,
            teacher
        ],
        [50, 22, 11, 17]
    );
    for call in calls {
        let target = module_path(manifest.nodes[call.to].path).unwrap();
        let source = read_source(&manifest.nodes[call.from]);
        let alias = binding(&target, &source).expect("reviewed Python import binding");
        assert!(call
            .expression
            .starts_with(&format!("{alias}.{}(", name(call.to))));
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn providers_pin_twenty_nine_local_sites() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let calls: Vec<_> = provider_calls(&manifest)
        .iter()
        .filter(|call| call.kind == "local")
        .collect();
    let count = |prefix| {
        calls
            .iter()
            .filter(|call| call.from.starts_with(prefix))
            .count()
    };
    assert_eq!(
        [calls.len(), count("changes."), count("tutor.engine.")],
        [29, 7, 8]
    );
    for call in calls {
        assert!(call.expression.starts_with(&format!("{}(", name(call.to))));
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn providers_refuse_wrong_nested_paths_and_aliases() {
    let input = combined_input();
    assert!(parse(&input.replacen(
        "src/careerbrew/jobs/strategy.py",
        "src/careerbrew/jobs.py",
        1
    ))
    .is_err());
    assert!(parse(&input.replacen(
        "src/careerbrew/jobs/strategy.py",
        "src/careerbrew/jobs/../jobs/strategy.py",
        1
    ))
    .is_err());
    let manifest = parse(&input).unwrap();
    let source = read_source(&manifest.nodes["jobs.strategy.build"]);
    assert_eq!(binding("tutor.engine", &source), Some("tutor"));
    let wrong = source.replace(
        "from ..tutor import engine as tutor",
        "from ..tutor import cards as tutor",
    );
    assert_eq!(binding("tutor.engine", &wrong), None);
}

#[test]
fn providers_keep_deferred_execution_boundaries() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let prior = parse(MANIFEST).unwrap();
    let new: BTreeSet<_> = manifest
        .unresolved
        .difference(&prior.unresolved)
        .copied()
        .collect();
    assert_eq!(
        new,
        BTreeSet::from([
            ("research.page", 29, "connectors.http_get(url)"),
            (
                "tutor.engine.mastery",
                184,
                "sched.get_card_retrievability("
            ),
            (
                "tutor.exercises._child",
                270,
                "[sys.executable, \"-I\", \"-c\", HARNESS]"
            ),
        ])
    );
    for (caller, line, expression) in new {
        check_anchor(&manifest, caller, line, expression);
    }
}

#[test]
fn providers_keep_snapshot_data_outside_call_graph() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let uses: BTreeSet<_> = manifest
        .data_uses
        .iter()
        .map(|site| {
            check_anchor(&manifest, site.caller, site.line, site.expression);
            (site.artifact, site.access, site.caller)
        })
        .collect();
    assert_eq!(
        uses,
        BTreeSet::from([
            ("strategy-snapshot", "write", "jobs.strategy.build"),
            ("strategy-snapshot", "read", "changes.current"),
            ("strategy-snapshot", "read", "teacher.next_step"),
        ])
    );
    assert!(!manifest.calls.iter().any(|call| matches!(
        (call.from, call.to),
        (
            "changes.current" | "teacher.next_step",
            "jobs.strategy.build"
        ) | (
            "jobs.strategy.build",
            "changes.current" | "teacher.next_step"
        )
    )));
}

#[test]
fn providers_add_no_multi_function_component() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let groups = components(&manifest, None);
    assert_eq!(groups.len(), manifest.nodes.len());
    assert!(groups.iter().all(|group| group.len() == 1));
}

#[test]
fn providers_detect_planted_strategy_tutor_cycle() {
    let input = combined_input();
    let manifest = parse(&input).unwrap();
    let groups = components(
        &manifest,
        Some(("tutor.engine.readiness", "jobs.strategy.build")),
    );
    let nontrivial: Vec<_> = groups.into_iter().filter(|group| group.len() > 1).collect();
    assert_eq!(
        nontrivial,
        [BTreeSet::from([
            "jobs.strategy.build",
            "tutor.engine.readiness"
        ])]
    );
}

fn workday_input() -> String {
    let workday = WORKDAY
        .strip_prefix("activation-manifest-v1\td003b413\tpartial\n")
        .expect("workday manifest revision and partial coverage");
    format!("{}{workday}", combined_input())
}

fn workday_calls<'a, 'm>(manifest: &'m Manifest<'a>) -> &'m [Call<'a>] {
    let prior = combined_input();
    let prior_count = parse(&prior).unwrap().calls.len();
    &manifest.calls[prior_count..]
}

fn has_import(source: &str, expected: &str) -> bool {
    source
        .lines()
        .any(|line| line.split_once('#').map_or(line, |(code, _)| code).trim() == expected)
}

fn workday_binding(target: &str, member: &str, named: bool, source: &str) -> bool {
    let imports: &[&str] = match (target, member, named) {
        ("calendar", "now", true) => &["from ..calendar import now"],
        ("workday.gate", "run_verify", true) => &["from .gate import run_verify"],
        ("workday.status", "stored" | "handled", true) => &["from .status import handled, stored"],
        ("workday.status", _, false) => &["from . import status"],
        ("workday.questions", _, false) => &["from . import questions"],
        ("workday.watch", _, false) => &["from . import plan, watch"],
        ("workday.plan", _, false) => &["from . import plan, watch", "from . import gate, plan"],
        ("workday.gate", _, false) => &["from . import gate, plan"],
        ("workday.end", _, false) => &[
            "from . import end",
            "from . import end, gate, questions, stages, start, status, steps, timing",
        ],
        ("day.watch", _, false) => &["from ..day import watch as day_watch"],
        ("day.log", _, false) => &[
            "from ..day import log as daylog",
            "from . import log as daylog",
        ],
        ("day.handoff", _, false) => &["from ..day import handoff"],
        ("day.pm_promises", _, false) => {
            &["from ..day import pm_promises", "from . import pm_promises"]
        }
        ("day.pm_learn", _, false) => &["from . import pm_learn"],
        ("work.sessions", _, false) => &["from ..work import sessions"],
        ("work.live", _, false) => &["from ..work import live"],
        _ if !named && !target.contains('.') => return true,
        _ => return false,
    };
    imports.iter().any(|import| has_import(source, import))
}

fn workday_expression(target: &str, member: &str, expression: &str, source: &str) -> bool {
    let named = expression.starts_with(&format!("{member}("));
    if !workday_binding(target, member, named, source) {
        return false;
    }
    if named {
        return true;
    }
    let alias = match target {
        "day.watch" => "day_watch",
        "day.log" => "daylog",
        _ => target.rsplit('.').next().unwrap(),
    };
    expression.starts_with(&format!("{alias}.{member}("))
}

#[test]
fn workday_pins_sixty_cross_module_sites() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let calls: Vec<_> = workday_calls(&manifest)
        .iter()
        .filter(|call| call.kind == "cross")
        .collect();
    let count = |prefix| {
        calls
            .iter()
            .filter(|call| call.from.starts_with(prefix))
            .count()
    };
    assert_eq!(
        [
            calls.len(),
            count("workday.start."),
            count("workday.end."),
            count("workday.watch."),
            count("day.watch."),
            count("workday.plan.")
        ],
        [60, 11, 28, 9, 4, 2]
    );
    assert_eq!(count("day.log.") + count("day.pm_"), 6);
    for call in calls {
        let target = module_path(manifest.nodes[call.to].path).unwrap();
        let source = read_source(&manifest.nodes[call.from]);
        assert!(
            workday_expression(&target, name(call.to), call.expression, &source),
            "wrong binding for {} to {}",
            call.from,
            call.to
        );
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn workday_pins_twenty_two_local_sites() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let calls: Vec<_> = workday_calls(&manifest)
        .iter()
        .filter(|call| call.kind == "local")
        .collect();
    let count = |prefix| {
        calls
            .iter()
            .filter(|call| call.from.starts_with(prefix))
            .count()
    };
    assert_eq!(
        [
            calls.len(),
            count("workday.start."),
            count("workday.end."),
            count("workday.watch."),
            count("day.watch.")
        ],
        [22, 6, 5, 3, 4]
    );
    assert_eq!(count("day.log.") + count("day.pm_"), 4);
    for call in calls {
        assert!(call.expression.starts_with(&format!("{}(", name(call.to))));
        check_anchor(&manifest, call.from, call.line, call.expression);
    }
}

#[test]
fn workday_refuses_wrong_watcher_alias_and_named_import() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let source = read_source(&manifest.nodes["workday.watch.run_pass"]);
    assert!(workday_expression(
        "day.watch",
        "running",
        "day_watch.running()",
        &source
    ));
    assert!(!workday_expression(
        "day.watch",
        "running",
        "watch.running()",
        &source
    ));
    let substituted = source.replace(
        "from ..day import watch as day_watch",
        "from ..day import log as day_watch",
    );
    assert!(!workday_expression(
        "day.watch",
        "running",
        "day_watch.running()",
        &substituted
    ));
    let source = read_source(&manifest.nodes["workday.end.finish"]);
    assert!(workday_expression(
        "workday.watch",
        "end",
        "watch.end()",
        &source
    ));
    assert!(workday_expression(
        "day.log",
        "save",
        "daylog.save(",
        &source
    ));
    let source = read_source(&manifest.nodes["workday.start.probe"]);
    let substituted = source.replace(
        "from .gate import run_verify",
        "from .gate import may_run as run_verify",
    );
    assert!(!workday_expression(
        "workday.gate",
        "run_verify",
        "run_verify(",
        &substituted
    ));
}

#[test]
fn workday_keeps_seven_runtime_boundaries() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let prior_input = combined_input();
    let prior = parse(&prior_input).unwrap();
    let new: BTreeSet<_> = manifest
        .unresolved
        .difference(&prior.unresolved)
        .copied()
        .collect();
    assert_eq!(
        new,
        BTreeSet::from([
            ("day.watch.run_cycle", 334, "fn(day)"),
            ("day.watch._rebuild", 307, "fn()"),
            ("workday.watch.run_pass", 460, "runner(s[\"cycle\"]"),
            ("workday.gate.run_verify", 62, "subprocess.run(argv"),
            ("day.watch.start", 496, "subprocess.Popen(cmd"),
            ("day.watch._reexec", 431, "os.execv(sys.executable, args)"),
            (
                "workday.start.preflight",
                253,
                "runner([\"sed\", \"--version\"])"
            ),
        ])
    );
    for (caller, line, expression) in new {
        check_anchor(&manifest, caller, line, expression);
    }
    check_anchor(
        &manifest,
        "day.watch._rebuild",
        301,
        "lambda: daylog.save(day)",
    );
    let source = read_source(&manifest.nodes["day.watch.run_cycle"]);
    assert!(source
        .lines()
        .nth(271)
        .unwrap()
        .contains("\"changes\": _src_changes"));
}

#[test]
fn workday_preserves_actual_watcher_call_direction() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let watcher_edges: BTreeSet<_> = workday_calls(&manifest)
        .iter()
        .filter(|call| {
            (call.from.starts_with("workday.") && call.to.starts_with("day.watch."))
                || (call.from.starts_with("day.watch.")
                    && (call.to.starts_with("workday.") || call.to.starts_with("changes.")))
        })
        .map(|call| (call.from, call.to))
        .collect();
    assert_eq!(
        watcher_edges,
        BTreeSet::from([
            ("workday.watch.run_pass", "day.watch.running"),
            ("workday.watch.end", "day.watch.stop"),
            ("day.watch._src_changes", "changes.check"),
        ])
    );
}

#[test]
fn workday_adds_no_multi_function_component() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let groups = components(&manifest, None);
    assert_eq!(groups.len(), manifest.nodes.len());
    assert!(groups.iter().all(|group| group.len() == 1));
}

#[test]
fn workday_detects_planted_five_function_cycle() {
    let input = workday_input();
    let manifest = parse(&input).unwrap();
    let groups = components(
        &manifest,
        Some(("day.watch.stop", "workday.watch.run_pass")),
    );
    let nontrivial: Vec<_> = groups.into_iter().filter(|group| group.len() > 1).collect();
    assert_eq!(
        nontrivial,
        [BTreeSet::from([
            "day.watch.stop",
            "workday.end.finish",
            "workday.end.tick",
            "workday.watch.end",
            "workday.watch.run_pass",
        ])]
    );
}
