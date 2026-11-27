//! `careerbrew.tutor.voice` and `cli.cmd_drill_speak` without `--aloud`.
//!
//! Speech-to-text runs only the configured local command. The audio copy and
//! the transcript stay in a temp folder that is removed after each call.
//! Practice records go through the [`Practice`] seam (rust-21 owns the native
//! teacher; the default backs onto the rust-20 tutor engine).

use crate::{data_root, field, py_str, repo_root, shlex, truthy, Error, Result};
use careerbrew_policy::text;
use careerbrew_vault::canon::Canon;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const STOP: [&str; 14] = [
    "about", "after", "their", "there", "these", "which", "would", "could", "should", "before",
    "least", "where", "while", "named",
];

fn voice_error(message: impl Into<String>) -> Error {
    Error::new("VoiceError", message)
}

/// The practice records voice grading reads and writes.
pub trait Practice {
    fn items(&self) -> Result<BTreeMap<String, Value>>;
    fn rubrics(&self) -> Result<BTreeMap<String, Value>>;
    fn record(
        &self,
        item: &str,
        met: &[String],
        seconds: Option<i64>,
        graded_by: &str,
    ) -> Result<Value>;
    fn feedback(&self, row: &Value) -> Result<Vec<String>>;
}

/// The rust-20 tutor engine over the data root and this checkout.
pub struct TutorPractice {
    root: PathBuf,
    repo: PathBuf,
    knowledge: Vec<(String, PathBuf)>,
    rubric_files: Vec<(String, PathBuf)>,
}

impl TutorPractice {
    pub fn open() -> Result<Self> {
        let root = data_root()?;
        let repo = repo_root();
        let knowledge = careerbrew_tutor::engine::resources(&root, &repo, "knowledge")?
            .into_iter()
            .filter(|(origin, _)| origin != "base")
            .collect();
        let rubric_files = careerbrew_tutor::engine::resources(&root, &repo, "rubrics")?;
        Ok(Self {
            root,
            repo,
            knowledge,
            rubric_files,
        })
    }
}

impl Practice for TutorPractice {
    fn items(&self) -> Result<BTreeMap<String, Value>> {
        Ok(careerbrew_tutor::engine::items(
            &self.root,
            &self.repo,
            &self.knowledge,
        )?)
    }

    fn rubrics(&self) -> Result<BTreeMap<String, Value>> {
        Ok(careerbrew_tutor::engine::rubrics(
            &self.root,
            &self.repo,
            &self.rubric_files,
        )?)
    }

    fn record(
        &self,
        item: &str,
        met: &[String],
        seconds: Option<i64>,
        graded_by: &str,
    ) -> Result<Value> {
        Ok(careerbrew_tutor::engine::record(
            &self.root,
            &self.repo,
            &self.knowledge,
            &self.rubric_files,
            item,
            met,
            careerbrew_tutor::engine::Attempt {
                seconds,
                at: careerbrew_tutor::engine::now(),
                graded_by,
            },
        )?)
    }

    fn feedback(&self, row: &Value) -> Result<Vec<String>> {
        Ok(careerbrew_tutor::engine::feedback(
            &self.root,
            &self.repo,
            &self.knowledge,
            &self.rubric_files,
            row,
        )?)
    }
}

fn setting(key: &str) -> Result<String> {
    let context = careerbrew_tutor::context()?;
    let settings = careerbrew_vault::settings::read_settings(&context.store())?;
    let settings = careerbrew_setup::value::from_canon(&settings);
    Ok(field(&settings, key)
        .filter(|value| truthy(Some(value)))
        .map(py_str)
        .unwrap_or_default())
}

/// `shutil.which(name)` over `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into());
    for dir in path.split(':') {
        let dir = if dir.is_empty() { "." } else { dir };
        let candidate = Path::new(dir).join(name);
        let Ok(meta) = std::fs::metadata(&candidate) else {
            continue;
        };
        if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
            let ok = std::ffi::CString::new(candidate.to_string_lossy().as_bytes())
                .map(|raw| unsafe { libc::access(raw.as_ptr(), libc::X_OK) } == 0)
                .unwrap_or(false);
            if ok {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn stt_command() -> Result<String> {
    let command = setting("stt_command")?;
    if !command.is_empty() {
        return Ok(command);
    }
    let model = setting("stt_model")?;
    if !model.is_empty() && which("whisper-cli").is_some() {
        let expanded = crate::pure_path(&crate::expand_user(&model).to_string_lossy());
        return Ok(format!(
            "whisper-cli -nt -np -m {} -f {{audio}}",
            shlex::quote(&expanded)
        ));
    }
    Ok(String::new())
}

struct Ran {
    code: i32,
    stdout: Vec<u8>,
}

fn run(argv: &[String], limit: Duration) -> std::io::Result<Ran> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut bytes);
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stderr, &mut bytes);
        bytes
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "speech command timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = out.join().unwrap_or_default();
    let _ = err.join();
    use std::os::unix::process::ExitStatusExt;
    let code = status
        .code()
        .unwrap_or_else(|| -status.signal().unwrap_or(0));
    Ok(Ran { code, stdout })
}

fn suffix_lower(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match name.rfind('.') {
        Some(at) if at > 0 && at + 1 < name.len() => name[at..].to_lowercase(),
        _ => String::new(),
    }
}

fn to_wav(src: &Path, tmp: &Path) -> Result<PathBuf> {
    if suffix_lower(src) == ".wav" || which("ffmpeg").is_none() {
        return Ok(src.to_path_buf());
    }
    let out = tmp.join("answer.wav");
    let argv = [
        "ffmpeg",
        "-loglevel",
        "error",
        "-y",
        "-i",
        &src.to_string_lossy(),
        "-ar",
        "16000",
        "-ac",
        "1",
        &out.to_string_lossy(),
    ]
    .map(str::to_owned);
    let done = run(&argv, Duration::from_secs(120))?;
    Ok(if done.code == 0 {
        out
    } else {
        src.to_path_buf()
    })
}

pub fn transcribe(audio: &Path) -> Result<String> {
    let command = stt_command()?;
    if command.is_empty() {
        return Err(voice_error("no local speech-to-text engine: install whisper.cpp and run `careerbrew config set stt_model <path to ggml model>`, or type your answer with --text"));
    }
    if !audio.is_file() {
        return Err(voice_error(format!(
            "no audio file at {}",
            crate::pure_path(&audio.to_string_lossy())
        )));
    }
    let tmp = tempfile::Builder::new()
        .prefix("careerbrew-voice-")
        .tempdir()?;
    let done = {
        let wav = to_wav(audio, tmp.path())?;
        let wav = wav.to_string_lossy().into_owned();
        let argv = shlex::split(&command)?
            .into_iter()
            .map(|part| part.replace("{audio}", &wav))
            .collect::<Vec<_>>();
        if argv.is_empty() {
            return Err(Error::new("IndexError", "list index out of range"));
        }
        match run(&argv, Duration::from_secs(300)) {
            Ok(done) => done,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(voice_error(format!(
                    "speech-to-text command not found: {} (check `careerbrew config`)",
                    argv[0]
                )));
            }
            Err(error) => return Err(error.into()),
        }
    };
    tmp.close()?;
    if done.code != 0 {
        return Err(voice_error(format!(
            "speech-to-text failed (exit {}); run the stt_command by hand to see why",
            done.code
        )));
    }
    let stdout = String::from_utf8(done.stdout)
        .map_err(|_| Error::new("UnicodeDecodeError", "engine output is not UTF-8"))?;
    let bracket = regex::Regex::new(r"\[[^\]]*\]").expect("valid pattern");
    let joined = text::split(&bracket.replace_all(&stdout, " ")).join(" ");
    if joined.is_empty() {
        return Err(voice_error(
            "the transcript is empty; speak closer to the microphone, or type your answer",
        ));
    }
    Ok(joined)
}

pub fn cues(criterion: &Value) -> Vec<String> {
    if let Some(cues) = field(criterion, "cues").filter(|value| truthy(Some(value))) {
        let items = cues.as_array().cloned().unwrap_or_default();
        return items.iter().map(|cue| text::lower(&py_str(cue))).collect();
    }
    let words = regex::Regex::new(r"[a-z][a-z-]{4,}").expect("valid pattern");
    let body = text::lower(&field(criterion, "text").map(py_str).unwrap_or_default());
    words
        .find_iter(&body)
        .map(|found| found.as_str().to_owned())
        .filter(|word| !STOP.contains(&word.as_str()))
        .collect()
}

fn ascii_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// `_says`: the cue with word boundaries on its alphanumeric ends.
pub fn says(said: &str, cue: &str) -> bool {
    let pre = cue.chars().next().is_some_and(text::is_alnum);
    let post = cue.chars().last().is_some_and(text::is_alnum);
    let mut from = 0;
    while let Some(at) = said[from..].find(cue).map(|n| n + from) {
        let before = said[..at].chars().last();
        let after = said[at + cue.len()..].chars().next();
        if (!pre || !ascii_word(before)) && (!post || !ascii_word(after)) {
            return true;
        }
        let step = said[at..].chars().next().map_or(1, char::len_utf8);
        from = at + step;
        if from > said.len() {
            break;
        }
    }
    cue.is_empty()
}

pub fn grade(practice: &dyn Practice, item_id: &str, answer: &str) -> Result<Vec<String>> {
    let items = practice.items()?;
    let Some(item) = items.get(item_id) else {
        return Err(Error::key(&format!(
            "unknown item {item_id} (see `careerbrew drill list`)"
        )));
    };
    let rubrics = practice.rubrics()?;
    let rubric_id = field(item, "rubric").map(py_str).unwrap_or_default();
    let rubric = rubrics
        .get(&rubric_id)
        .ok_or_else(|| Error::key(&rubric_id))?;
    let tokens = regex::Regex::new(r"[a-z0-9%()^'-]+").expect("valid pattern");
    let lowered = text::lower(answer);
    let said = tokens
        .find_iter(&lowered)
        .map(|found| found.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(field(rubric, "criteria")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|c| cues(c).iter().any(|cue| says(&said, cue)))
        .map(|c| field(c, "id").map(py_str).unwrap_or_default())
        .collect())
}

pub fn answer(
    practice: &dyn Practice,
    item_id: &str,
    typed: Option<&str>,
    audio: Option<&Path>,
    seconds: Option<i64>,
) -> Result<Value> {
    let (said, source) = if let Some(audio) = audio {
        (transcribe(audio)?, "audio")
    } else if let Some(typed) = typed.filter(|t| !text::strip(t).is_empty()) {
        (typed.to_owned(), "typed")
    } else {
        return Err(voice_error(
            "give an answer: --audio <file> or --text <answer>",
        ));
    };
    let met = grade(practice, item_id, &said)?;
    let row = practice.record(item_id, &met, seconds, &format!("voice-{source}"))?;
    let score = field(&row, "score").and_then(Value::as_f64).unwrap_or(0.0);
    careerbrew_tutor::emit(
        &data_root()?,
        "tutor.voice",
        vec![
            ("status", Canon::Str(source.into())),
            ("words", Canon::Int(text::split(&said).len().into())),
            ("score", Canon::Float(score)),
        ],
    )?;
    let feedback = practice.feedback(&row)?;
    Ok(json!({"transcript": said, "source": source, "row": row, "feedback": feedback}))
}

/// `cli.cmd_drill_speak` without `--aloud`: (exit code, printed text).
pub fn drill_speak(
    practice: &dyn Practice,
    item_id: &str,
    typed: Option<&str>,
    audio: Option<&str>,
    seconds: Option<i64>,
) -> Result<(i32, String)> {
    let terms = careerbrew_feed::sources::sensitive_terms(&data_root()?)?;
    let mut printed = String::new();
    let mut say = |line: &str| {
        printed.push_str(&careerbrew_policy::redact(line, &terms));
        printed.push('\n');
    };
    let items = practice.items()?;
    let Some(item) = items.get(item_id) else {
        say(&format!(
            "unknown item {item_id} (see `careerbrew drill list`)"
        ));
        return Ok((1, printed));
    };
    say(&format!(
        "Question: {}",
        field(item, "prompt").map(py_str).unwrap_or_default()
    ));
    let audio = audio.map(crate::expand_user);
    let out = match answer(practice, item_id, typed, audio.as_deref(), seconds) {
        Ok(out) => out,
        Err(error) if error.class == "VoiceError" => {
            say(&format!("error: {}", error.message));
            return Ok((1, printed));
        }
        Err(error) => return Err(error),
    };
    if out["source"] == "audio" {
        say(&format!("Transcript: {}", py_str(&out["transcript"])));
    }
    let met = out["row"]["met"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(py_str)
        .collect::<Vec<_>>()
        .join(", ");
    say(&format!(
        "Met: {}",
        if met.is_empty() { "none" } else { &met }
    ));
    for line in out["feedback"].as_array().cloned().unwrap_or_default() {
        say(&py_str(&line));
    }
    Ok((0, printed))
}
