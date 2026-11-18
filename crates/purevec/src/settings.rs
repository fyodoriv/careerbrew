//! Set `settings`: the pure value logic of `src/careerbrew/config.py` over an
//! in-memory settings map. It holds the 13 self-contained validators, the
//! `timezone`, `stt_model` and `pm_send_via` lambdas, `DEFAULTS` in order
//! (with the 8 generated `time_baseline_*` keys), `effective` precedence,
//! `get`, the `set_value` error texts and the `unset` rule for a stored
//! `None`. Ops: `effective`, `get`, `set`, `unset`. The file read and write,
//! the event, and the validators that call workday modules or tzdata
//! (`work_steps`, `work_checkouts`, `work_signin_probe`, `standup_tz`) are not
//! ported: `set` answers `{"refused": "delegated"}` for those keys.
//!
//! The matchers are hand-written and follow Python's `re` on `str`: `\d` is
//! any Unicode decimal digit (category Nd) and `\s` is `str.isspace()`.
use serde_json::{json, Value};

/// A default value.
enum D {
    Str(&'static str),
    Int(i128),
    Bool(bool),
    Ints(&'static [i128]),
    Strs(&'static [&'static str]),
}

/// A validator.
enum P {
    Text,
    Hhmm,
    Int(i128, i128),
    Days,
    List,
    Bool,
    Email,
    Priority,
    Stt,
    Quiet,
    ArtifactUrl,
    ToolSources,
    Choice(&'static [&'static str]),
    Same,
    PmSendVia,
    Delegated,
}

/// `DEFAULTS` before the generated `time_baseline_*` keys: key, default,
/// validator, help, in insertion order.
const DEFAULTS: &[(&str, D, P, &str)] = &[
    ("ladder", D::Str("generic-swe"), P::Text, "promotion ladder id (a company plugin sets its own)"),
    ("target_level", D::Str("senior"), P::Text, "the level you are working toward, a level of the ladder"),
    ("work_start", D::Str("09:00"), P::Hhmm, "work day start, local time"),
    ("work_end", D::Str("17:30"), P::Hhmm, "work day end, local time"),
    ("work_wrap_minutes", D::Int(30), P::Int(0, 240), "the /work day switches to wrap-up this many minutes before work_end: the loops keep running, tomorrow's handoff is written, and one reminder pops up (with notify_events on); at work_end the day ends by itself"),
    ("work_days", D::Ints(&[0, 1, 2, 3, 4]), P::Days, "workdays, Monday=0"),
    ("timezone", D::Str(""), P::Same, "IANA time zone; empty means the system one"),
    ("backup_targets", D::Strs(&[]), P::List, "backup folders, comma-separated (empty: ~/Backups/careerbrew)"),
    ("backup_icloud", D::Bool(false), P::Bool, "also write each encrypted backup to iCloud Drive/careerbrew (opt-in; refused on a corporate machine)"),
    ("serve_port", D::Int(8765), P::Int(1024, 65535), "port of the local web app"),
    ("web_cache_seconds", D::Int(300), P::Int(0, 3600), "web views answer a repeat from memory until a data file, a setting, or the clock changes, at most this long (calendar feeds); 0: off"),
    ("web_token_max_days", D::Int(30), P::Int(1, 365), "the web app's sign-in token lasts this many days across restarts, then the next `careerbrew open` makes a new one (`careerbrew web rotate-token` does it now)"),
    ("web_tab_cache", D::Bool(true), P::Bool, "each browser tab keeps the last data of every view it showed, encrypted with a key only the running app holds, so a reload shows it at once (off: memory only, a reload loads again)"),
    ("start_watch", D::Bool(true), P::Bool, "`careerbrew` with no arguments turns the day watcher on"),
    ("start_web", D::Bool(true), P::Bool, "`careerbrew` with no arguments starts the web app in the background"),
    ("questions_per_day", D::Int(5), P::Int(0, 50), "most open questions careerbrew asks at once"),
    ("inbox_group_min", D::Int(3), P::Int(2, 100), "this many or more alike suggestions (same kind, project, and source) show as one group with Accept all, Review each, and Dismiss all"),
    ("inbox_expire_done", D::Bool(true), P::Bool, "hide a suggestion when data careerbrew already reads shows it done elsewhere (a PR merged, a review you gave, CI green); an unsure match only says maybe done; Bring back undoes it"),
    ("first_run_checklist", D::Bool(true), P::Bool, "show the Getting started checklist on Dashboard until every step is done (false: hide it)"),
    ("run_pages", D::Bool(true), P::Bool, "long runs (scripts/checkup.sh, `careerbrew`, review, work collect, day plan, day render, checkup, machine ready, machine new-user, plugin new) end with an HTML page (done, blockers, next) and print only its path and one summary line (off: their step text)"),
    ("pm_report_title", D::Str("Daily update"), P::Text, "subject and first line of the PM report"),
    ("metrics_share", D::Bool(false), P::Bool, "write a weekly numbers-only metrics file for agentbrew's metrics share (opt-in)"),
    ("telemetry_consent", D::Str("unset"), P::Choice(&["unset", "yes", "no"]), "your answer about anonymous usage counts to PostHog EU: unset (nothing is sent), yes, or no; change it with careerbrew telemetry on | off"),
    ("checkup_artifact_url", D::Str(""), P::ArtifactUrl, "this machine's repo health Dashboard on claude.ai: /careerbrew-checkup refreshes it from `careerbrew report public` (empty: the next checkup creates one)"),
    ("pm_send_via", D::Str("slack"), P::PmSendVia, "how Send delivers the PM update: slack (posts it) or email (opens a draft)"),
    ("pm_slack", D::Str(""), P::Text, "Slack channel or person for the PM update: a channel id, #channel, or a member id (U...)"),
    ("pm_email", D::Str(""), P::Email, "your PM's address when pm_send_via is email: Send opens a draft to it"),
    ("project_priority", D::Strs(&[]), P::Priority, "projects by priority, rank 1 first: name=glob|glob, ... (globs match repos, ticket keys, titles); the PM update, plan, and Next step follow it"),
    ("feed_keep_days", D::Int(30), P::Int(1, 365), "days the work feed keeps each original event (encrypted); its analysis (triage, personal flag) stays"),
    ("feed_tool_sources", D::Strs(&[]), P::ToolSources, "MCP tools whose replies `work feed transcript` stores from an agent session, glob=source, comma-separated (for example mcp__chat__*=chat, mcp__tickets__*=tickets)"),
    ("live_source_max_age_minutes", D::Int(15), P::Int(1, 10080), "maximum acquisition age in minutes for a validated live-source reply; a source max_age_minutes overrides it"),
    ("live_open_links", D::Bool(false), P::Bool, "the live cycle opens the links in kept events to summarize them (off: each link waits for your agent); never private or local addresses, at most 1 MB per page"),
    ("feed_ask_overdue_hours", D::Int(8), P::Int(1, 168), "work hours (work_start to work_end on work_days) an open ask waits before Today and `work feed show` mark it overdue"),
    ("project_memory_sync", D::Bool(true), P::Bool, "a project's memory file edit updates the project (links, trackers, status), and matching work-tool items go into it as dated notes"),
    ("stt_command", D::Str(""), P::Stt, "local speech-to-text command for voice practice; {audio} is the file, stdout is the transcript (empty: whisper-cli with stt_model)"),
    ("stt_model", D::Str(""), P::Same, "path to a whisper.cpp model file (ggml-*.bin) used when stt_command is empty"),
    ("notify_events", D::Bool(false), P::Bool, "notify me about important new events: pages, direct messages, review requests, mentions, high-priority suggestions (opt-in)"),
    ("notify_min_priority", D::Int(4), P::Int(1, 5), "notify only at or above this priority: 5 a page on you, 4 a DM or review request, 3 a mention, important mail, or suggestion (+1 for your rank-1 project)"),
    ("notify_quiet_hours", D::Str("19:00-08:00"), P::Quiet, "no pop-ups between these local times, HH:MM-HH:MM (none: no quiet hours); held ones arrive together after"),
    ("notify_daily_cap", D::Int(6), P::Int(0, 50), "most pop-ups a day; past it, new ones wait in the web app's Notifications view"),
    ("notify_batch_minutes", D::Int(10), P::Int(0, 240), "new events within this many minutes of the last pop-up arrive together in the next one"),
    ("notify_snooze_minutes", D::Int(60), P::Int(5, 1440), "Snooze hides a notification for this many minutes, then it comes back"),
    ("notify_backend", D::Str("auto"), P::Choice(&["auto", "alerter", "terminal-notifier", "osascript"]), "how pop-ups show: auto picks alerter (buttons), then terminal-notifier (click opens), then osascript (plain)"),
    ("standup_days", D::Ints(&[0, 1, 2, 3, 4]), P::Days, "standup weekdays, Monday=0; a Monday report counts from the last standup day of the week before"),
    ("standup_time", D::Str("10:00"), P::Hhmm, "standup start, HH:MM in standup_tz"),
    ("standup_tz", D::Str(""), P::Delegated, "IANA time zone of the standup and of evidence times with no offset (empty: the timezone setting)"),
    ("standup_window", D::Str("previous-day"), P::Choice(&["previous-day", "since-last"]), "where the standup report starts: previous-day (the previous standup day's standup, all day long) or since-last (the last standup that has ended, today's too)"),
    ("standup_minutes", D::Int(15), P::Int(1, 240), "standup length; a report run during the standup still counts from the one before"),
    ("standup_proof_minutes", D::Int(60), P::Int(1, 1440), "how old a live proof passed with `work standup --proof` may be at each build; an older one fails its row (issue #271)"),
    ("agent_quiet_minutes", D::Int(60), P::Int(1, 1440), "a background agent's run whose last `work agent event` is running and older than this shows as quiet in Agent activity (issue #272)"),
    ("work_session_cap", D::Int(3), P::Int(1, 10), "most project sessions live at once on /work (`work session open`); one per project, reused all day (issue #274)"),
    ("work_claim_minutes", D::Int(30), P::Int(1, 480), "minutes a project session's claim on a shared thread or draft lasts after it was last taken (`work claim take`, issue #274)"),
    ("agent_keep_days", D::Int(7), P::Int(1, 90), "days Agent activity keeps each background agent's events (`work agent event`, issue #272)"),
    ("outlook_client_id", D::Str(""), P::Text, "the app registration (client ID) `careerbrew connect outlook` signs in with: your company's approved app; empty: a company plugin's connector_defaults.outlook.client_id"),
    ("outlook_tenant", D::Str(""), P::Text, "the Microsoft tenant for Outlook sign-in (a tenant ID or domain; empty: the plugin's, else organizations)"),
    ("work_steps", D::Strs(&[]), P::Delegated, "your own `/work` steps before plugin steps: comma-separated names or a JSON list of names and mappings (docs/plugins.md)"),
    ("work_step_timeout", D::Int(30), P::Int(1, 1440), "minutes a `/work` step may run; `careerbrew day check` then retries it once, and marks it failed after the second timeout (a plugin step's `timeout:` overrides it)"),
    ("work_slow_percent", D::Int(200), P::Int(110, 1000), "a finished `/work` step that took more than this percent of its median (its done runs on earlier days) is flagged slow on the day's page (issue #263)"),
    ("work_checkouts", D::Strs(&["careerbrew"]), P::Delegated, "checkouts `careerbrew day start` fast-forwards before the first `/work` step, besides each plugin's folder and each step's source: careerbrew (its own checkout) and full paths, comma-separated"),
    ("work_signin_probe", D::Str(""), P::Delegated, "a command `careerbrew day start` runs to check the shared browser is signed in (exit 0: signed in); when it fails, the day gets one sign-in question and the sign-in steps run last (empty: no probe)"),
    ("standup_me", D::Strs(&[]), P::List, "names, handles, or emails that mean you in a standup row's owner (owner `me` always does); your rows show when they have an update since the last standup"),
];

/// The actions whose `time_baseline_<action>` keys follow `DEFAULTS`.
const BASELINE_ACTIONS: &[&str] = &[
    "pm_update",
    "standup",
    "day_log",
    "day_plan",
    "evidence",
    "promo_doc",
    "job_scan",
    "inbox",
];

/// The first code point of each run of ten Unicode decimal digits (category
/// Nd, Unicode 15.0.0, the version of the oracle's Python 3.12).
const DIGIT_ZEROS: &[u32] = &[
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
    0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
    0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0, 0x11650,
    0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16a60, 0x16ac0,
    0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e950,
    0x1fbf0,
];

/// Python's `\d`: the digit's value when `c` is a Unicode decimal digit.
fn digit(c: char) -> Option<u32> {
    let code = c as u32;
    DIGIT_ZEROS
        .iter()
        .find(|zero| (**zero..**zero + 10).contains(&code))
        .map(|zero| code - zero)
}

/// Python's `str.isspace()`, which `\s` and `strip()` use.
fn space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn strip(v: &str) -> &str {
    v.trim_matches(space)
}

/// Python's `repr()` of a string. Non-ASCII code points are printed as they
/// are, except the C1 controls, the Unicode spaces and a few format
/// characters; other non-printable code points are a known gap.
fn repr(v: &str) -> String {
    let quote = if v.contains('\'') && !v.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in v.chars() {
        let code = c as u32;
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if c == quote => {
                out.push('\\');
                out.push(c);
            }
            _ if code < 0x20 || (0x7f..=0xa0).contains(&code) || code == 0xad => {
                out.push_str(&format!("\\x{code:02x}"))
            }
            _ if code > 0xff
                && (space(c)
                    || (0x200b..=0x200f).contains(&code)
                    || (0x202a..=0x202e).contains(&code)
                    || (0x2060..=0x206f).contains(&code)
                    || code == 0xfeff) =>
            {
                out.push_str(&format!("\\u{code:04x}"))
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `int(v)` in base 10: surrounding whitespace, one sign, Unicode
/// digits and single underscores between digits. `Ok(None)` is a valid
/// number too large for `i128`.
fn int(v: &str) -> Result<Option<i128>, String> {
    let bad = || format!("invalid literal for int() with base 10: {}", repr(v));
    // `int` maps Unicode spaces to ASCII and strips those; unlike
    // `str.strip()`, it keeps U+001C to U+001F.
    let text = v.trim_matches(char::is_whitespace);
    let (negative, body) = match text.chars().next() {
        Some('-') => (true, &text[1..]),
        Some('+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: Option<i128> = Some(0);
    let mut last_digit = false;
    for c in body.chars() {
        if c == '_' {
            if !last_digit {
                return Err(bad());
            }
            last_digit = false;
            continue;
        }
        let d = digit(c).ok_or_else(bad)?;
        value = value
            .and_then(|n| n.checked_mul(10))
            .and_then(|n| n.checked_add(i128::from(d)));
        last_digit = true;
    }
    if !last_digit {
        return Err(bad());
    }
    Ok(value.map(|n| if negative { -n } else { n }))
}

/// `([01]\d|2[0-3]):[0-5]\d` on exactly five code points.
fn hhmm(c: &[char]) -> bool {
    c.len() == 5
        && match c[0] {
            '0' | '1' => digit(c[1]).is_some(),
            '2' => ('0'..='3').contains(&c[1]),
            _ => false,
        }
        && c[2] == ':'
        && ('0'..='5').contains(&c[3])
        && digit(c[4]).is_some()
}

fn s(v: &str) -> Value {
    json!({"t": "str", "v": v})
}

fn i(v: i128) -> Value {
    json!({"t": "int", "v": v.to_string()})
}

fn list(items: Vec<Value>) -> Value {
    json!({"t": "list", "v": items})
}

fn int_range(v: &str, lo: i128, hi: i128) -> Result<Value, String> {
    match int(v)? {
        Some(n) if (lo..=hi).contains(&n) => Ok(i(n)),
        _ => Err(format!("use a whole number from {lo} to {hi}")),
    }
}

fn days(v: &str) -> Result<Value, String> {
    let mut found = Vec::new();
    for x in v.replace(' ', "").split(',').filter(|x| !x.is_empty()) {
        found.push(int(x)?);
    }
    let bad = "use weekday numbers 0-6 (Monday=0), comma-separated, for example 0,1,2,3,4";
    let mut days = Vec::new();
    for d in found {
        match d {
            Some(n) if (0..=6).contains(&n) => days.push(n),
            _ => return Err(bad.into()),
        }
    }
    days.sort_unstable();
    days.dedup();
    if days.is_empty() {
        return Err(bad.into());
    }
    Ok(list(days.into_iter().map(i).collect()))
}

fn split_list(v: &str) -> Vec<&str> {
    v.split(',').map(strip).filter(|x| !x.is_empty()).collect()
}

fn email(v: &str) -> Result<Value, String> {
    let ok = || {
        let (local, domain) = v.split_once('@')?;
        let plain = |c: char| c != '@' && !space(c);
        let domain: Vec<char> = domain.chars().collect();
        let dot = (1..domain.len().saturating_sub(1)).any(|at| domain[at] == '.');
        Some(
            !local.is_empty()
                && local.chars().all(plain)
                && domain.iter().all(|c| plain(*c))
                && dot,
        )
    };
    if !v.is_empty() && ok() != Some(true) {
        return Err("use an email address, or empty".into());
    }
    Ok(s(v))
}

fn text(v: &str) -> Result<Value, String> {
    if v.is_empty() || v.chars().count() > 120 || v.contains('\n') {
        return Err("use one line of at most 120 characters".into());
    }
    Ok(s(v))
}

/// `[first][rest]{0,max}` over ASCII classes.
fn name_like(v: &str, max: usize, rest: impl Fn(char) -> bool) -> bool {
    let mut chars = v.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && v.chars().count() <= max + 1
        && chars.all(rest)
}

fn priority(v: &str) -> Result<Value, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in v.split(',').map(strip) {
        if raw.is_empty() {
            continue;
        }
        let (name, eq, pats) = match raw.split_once('=') {
            Some((name, pats)) => (strip(name), true, strip(pats)),
            None => (strip(raw), false, ""),
        };
        let globs: Vec<&str> = pats
            .split('|')
            .map(strip)
            .filter(|g| !g.is_empty())
            .collect();
        if !name_like(name, 39, |c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
        }) {
            return Err(format!("project {}: use lowercase letters, digits, and dashes (max 40), for example tools=*/repo-a|*/repo-b", repr(name)));
        }
        if out
            .iter()
            .any(|x| x.split_once('=').map_or(x.as_str(), |(n, _)| n) == name)
        {
            return Err(format!(
                "project {name} is listed twice; each project has one rank"
            ));
        }
        if eq && globs.is_empty() {
            return Err(format!(
                "project {name}: no pattern after '=' (fix: {name}=PROJ-* or just {name})"
            ));
        }
        if globs
            .iter()
            .any(|g| g.chars().any(|c| space(c) || c == '=') || g.chars().count() > 80)
        {
            return Err(format!(
                "project {name}: a pattern has no spaces or '=' and at most 80 characters"
            ));
        }
        out.push(if globs.is_empty() {
            name.to_owned()
        } else {
            format!("{name}={}", globs.join("|"))
        });
    }
    if out.len() > 20 {
        return Err("at most 20 projects".into());
    }
    Ok(list(out.iter().map(|x| s(x)).collect()))
}

fn quiet(v: &str) -> Result<Value, String> {
    let v = strip(v);
    if ["", "none", "off"].contains(&v.to_lowercase().as_str()) {
        return Ok(s(""));
    }
    let c: Vec<char> = v.chars().collect();
    if !(c.len() == 11 && hhmm(&c[..5]) && c[5] == '-' && hhmm(&c[6..])) {
        return Err("use HH:MM-HH:MM, for example 19:00-08:00 (or none for no quiet hours)".into());
    }
    if c[..5] == c[6..] {
        return Err("the start and the end are the same time; use two different times, or none for no quiet hours".into());
    }
    Ok(s(v))
}

fn artifact_url(v: &str) -> Result<Value, String> {
    let id = v
        .strip_prefix("https://claude.ai/code/artifact/")
        .or_else(|| v.strip_prefix("https://claude.ai/artifact/"));
    let ok = id.is_some_and(|id| {
        (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    if !v.is_empty() && !ok {
        return Err("use a claude.ai artifact link, for example https://claude.ai/artifact/AbC123xyz, or empty".into());
    }
    Ok(s(v))
}

fn head40(v: &str) -> String {
    v.chars().take(40).collect()
}

fn tool_sources(v: &str) -> Result<Value, String> {
    let mut out = Vec::new();
    for raw in v.split(',').map(strip) {
        if raw.is_empty() {
            continue;
        }
        let (glob, src) = match raw.split_once('=') {
            Some((glob, src)) if !strip(glob).is_empty() && !strip(src).is_empty() => {
                (strip(glob), strip(src))
            }
            _ => {
                return Err(format!(
                    "{}: use glob=source, for example mcp__chat__*=chat",
                    repr(&head40(raw))
                ))
            }
        };
        if !name_like(src, 39, |c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
        }) {
            return Err(format!(
                "source {} must be a short lowercase name such as chat or tickets",
                repr(&head40(src))
            ));
        }
        out.push(s(&format!("{glob}={src}")));
    }
    Ok(list(out))
}

fn parse(parser: &P, v: &str) -> Result<Value, String> {
    match parser {
        P::Text => text(v),
        P::Hhmm => {
            let c: Vec<char> = v.chars().collect();
            if !hhmm(&c) {
                return Err("use HH:MM, for example 09:00".into());
            }
            Ok(s(v))
        }
        P::Int(lo, hi) => int_range(v, *lo, *hi),
        P::Days => days(v),
        P::List => Ok(list(split_list(v).into_iter().map(s).collect())),
        P::Bool => match v.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(json!({"t": "bool", "v": true})),
            "false" | "no" | "off" | "0" => Ok(json!({"t": "bool", "v": false})),
            _ => Err("use true or false".into()),
        },
        P::Email => email(v),
        P::Priority => priority(v),
        P::Stt => {
            if !v.is_empty() && !v.contains("{audio}") {
                return Err("the command needs an {audio} placeholder, for example: whisper-cli -nt -np -m ~/models/ggml-base.en.bin -f {audio}".into());
            }
            Ok(s(v))
        }
        P::Quiet => quiet(v),
        P::ArtifactUrl => artifact_url(v),
        P::ToolSources => tool_sources(v),
        P::Choice(options) => {
            if !options.contains(&v) {
                return Err(format!("use one of {}", options.join(", ")));
            }
            Ok(s(v))
        }
        P::Same => Ok(s(v)),
        P::PmSendVia => match v {
            "slack" | "email" => Ok(s(v)),
            _ => Err("use slack or email".into()),
        },
        P::Delegated => unreachable!("delegated keys are refused before parsing"),
    }
}

/// One `DEFAULTS` row, owned so the generated keys fit.
struct Row {
    key: String,
    default: Value,
    parser: &'static P,
    help: String,
}

const BASELINE: P = P::Int(0, 600);

fn rows() -> Vec<Row> {
    let encode = |d: &D| match d {
        D::Str(v) => s(v),
        D::Int(v) => i(*v),
        D::Bool(v) => json!({"t": "bool", "v": v}),
        D::Ints(v) => list(v.iter().map(|n| i(*n)).collect()),
        D::Strs(v) => list(v.iter().map(|x| s(x)).collect()),
    };
    let mut rows: Vec<Row> = DEFAULTS
        .iter()
        .map(|(key, default, parser, help)| Row {
            key: (*key).to_owned(),
            default: encode(default),
            parser,
            help: (*help).to_owned(),
        })
        .collect();
    rows.extend(BASELINE_ACTIONS.iter().map(|action| Row {
        key: format!("time_baseline_{action}"),
        default: i(0),
        parser: &BASELINE,
        help: format!(
            "minutes {action} takes you by hand, timed once (0: use the labeled estimate)"
        ),
    }));
    rows
}

/// The stored settings map as `[key, value]` canon pairs.
fn stored(input: &Value) -> Result<Vec<(Value, Value)>, String> {
    let map = match input.get("settings") {
        None => return Ok(Vec::new()),
        Some(map) if map["t"] == "map" => map,
        Some(_) => return Err("settings is not a map".into()),
    };
    map["v"]
        .as_array()
        .ok_or("settings is not a map")?
        .iter()
        .map(|pair| match pair.as_array().map(Vec::as_slice) {
            Some([k, v]) => Ok((k.clone(), v.clone())),
            _ => Err("settings pair is not a pair".to_owned()),
        })
        .collect()
}

fn lookup<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    map.iter().find(|(k, _)| *k == s(key)).map(|(_, v)| v)
}

fn as_map(map: &[(Value, Value)]) -> Value {
    json!({"t": "map", "v": map.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()})
}

fn raised(class: &str, message: String) -> Value {
    json!({"raised": class, "message": message})
}

fn row(key: Value, value: Value, source: &str, default: Value, help: &str) -> Value {
    json!({"t": "map", "v": [
        [s("key"), key],
        [s("value"), value],
        [s("source"), s(source)],
        [s("default"), default],
        [s("help"), s(help)],
    ]})
}

/// Maps one `settings` vector.
pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    let map = stored(input)?;
    let rows = rows();
    let key = || input["key"].as_str().ok_or("no key");
    let known = |key: &str| rows.iter().find(|row| row.key == key);
    match op {
        "effective" => {
            let mut out: Vec<Value> = rows
                .iter()
                .map(|r| {
                    let found = lookup(&map, &r.key);
                    let source = if found.is_some() {
                        "settings.yaml"
                    } else {
                        "default"
                    };
                    let value = found.unwrap_or(&r.default).clone();
                    row(s(&r.key), value, source, r.default.clone(), &r.help)
                })
                .collect();
            out.extend(
                map.iter()
                    .filter(|(k, _)| {
                        !(k["t"] == "str" && k["v"].as_str().is_some_and(|k| known(k).is_some()))
                    })
                    .map(|(k, v)| {
                        row(
                            k.clone(),
                            v.clone(),
                            "settings.yaml",
                            json!({"t": "null"}),
                            "not a known setting (kept as is)",
                        )
                    }),
            );
            Ok(Value::Array(out))
        }
        "get" => {
            let key = key()?;
            let Some(r) = known(key) else {
                return Ok(raised(
                    "KeyError",
                    format!("unknown setting {key}; see `careerbrew config`"),
                ));
            };
            Ok(json!({"value": lookup(&map, key).unwrap_or(&r.default)}))
        }
        "set" => {
            let key = key()?;
            let raw = input["raw"].as_str().ok_or("no raw value")?;
            let Some(r) = known(key) else {
                let names: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
                return Ok(raised(
                    "KeyError",
                    format!("unknown setting {key}; known: {}", names.join(", ")),
                ));
            };
            if matches!(r.parser, P::Delegated) {
                return Ok(json!({"refused": "delegated"}));
            }
            let value = match parse(r.parser, raw) {
                Ok(value) => value,
                Err(message) => return Ok(raised("ValueError", format!("{key}: {message}"))),
            };
            let mut written = map.clone();
            match written.iter_mut().find(|(k, _)| *k == s(key)) {
                Some(entry) => entry.1 = value.clone(),
                None => written.push((s(key), value.clone())),
            }
            Ok(json!({"value": value, "written": as_map(&written)}))
        }
        "unset" => {
            let key = key()?;
            // `data.pop(key, None) is not None`: a stored None is never removed.
            match lookup(&map, key) {
                Some(v) if v["t"] != "null" => {
                    let rest: Vec<(Value, Value)> =
                        map.iter().filter(|(k, _)| *k != s(key)).cloned().collect();
                    Ok(json!({"written": as_map(&rest)}))
                }
                _ => Ok(json!({"written": null})),
            }
        }
        _ => Err("unknown op".into()),
    }
}
