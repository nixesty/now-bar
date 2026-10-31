//! What Claude Code is doing, from its hooks: the step (tool and target), the task list, the
//! files it changed, its last answer and a permission it waits for. The pure part; the hook
//! process (`nowbar.exe claude-hook`) builds a `ClaudeNote` and the Now Bar keeps a `ClaudeInfo`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trf};

/// `s` at most `n` characters, a cut one ending in "…".
fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n - 1).chain(['…']).collect()
}

fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

/// "app.rs" from either kind of path.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(path)
}

fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

fn text(input: &Value, key: &str) -> String {
    input[key].as_str().unwrap_or("").to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Read,
    Edit,
    Write,
    Run,
    Search,
    Agent,
    Tool,
}

/// One tool call: what kind and on what ("app.rs", "cargo test", "paint_shelf").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub kind: StepKind,
    pub target: String,
}

pub fn step_of(tool: &str, input: &Value) -> Step {
    let path = || file_name(input["file_path"].as_str().or(input["notebook_path"].as_str()).unwrap_or("")).to_string();
    let (kind, target) = match tool {
        "Read" => (StepKind::Read, path()),
        "Edit" | "MultiEdit" | "NotebookEdit" => (StepKind::Edit, path()),
        "Write" => (StepKind::Write, path()),
        "Bash" | "PowerShell" => {
            let what = text(input, "description");
            (StepKind::Run, if what.trim().is_empty() { first_line(&text(input, "command")).to_string() } else { what })
        }
        "Grep" | "Glob" => (StepKind::Search, text(input, "pattern")),
        "WebSearch" => (StepKind::Search, text(input, "query")),
        "WebFetch" => (StepKind::Read, host_of(input["url"].as_str().unwrap_or("")).to_string()),
        "Agent" | "Task" => (StepKind::Agent, text(input, "description")),
        // mcp__server__tool: the tool's own name says enough.
        _ => (StepKind::Tool, tool.rsplit("__").next().unwrap_or(tool).to_string()),
    };
    Step { kind, target: cut(&target, 120) }
}

/// "Düzenliyor: app.rs", at most 60 characters.
pub fn step_label(s: &Step) -> String {
    let verb = match s.kind {
        StepKind::Read => tr("Okuyor"),
        StepKind::Edit => tr("Düzenliyor"),
        StepKind::Write => tr("Yazıyor"),
        StepKind::Run => tr("Çalıştırıyor"),
        StepKind::Search => tr("Arıyor"),
        StepKind::Agent => tr("Alt ajan"),
        StepKind::Tool => tr("Araç"),
    };
    cut(&format!("{verb}: {}", s.target.trim()), 60)
}

/// The file a tool changed (what "9 dosya" counts); reads change nothing.
pub fn file_of(tool: &str, input: &Value) -> Option<String> {
    match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => input["file_path"].as_str().or(input["notebook_path"].as_str()).map(str::to_string),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub content: String,
    pub status: TaskStatus,
}

/// A change to the task list: Claude Code's TodoWrite and the desktop app's plan tool send the
/// whole list; the plan tool also moves along it (`advance`, `done` and `start` are 1-based).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskChange {
    Replace(Vec<Task>),
    Moves { advance: bool, done: Vec<usize>, start: Option<usize> },
}

fn status_of(s: &str) -> TaskStatus {
    match s {
        "completed" => TaskStatus::Done,
        "in_progress" => TaskStatus::Active,
        _ => TaskStatus::Pending,
    }
}

fn task_list(items: &Value) -> Option<Vec<Task>> {
    let list = items.as_array()?;
    Some(list.iter().take(40).map(|t| Task { content: cut(&text(t, "content"), 80), status: status_of(t["status"].as_str().unwrap_or("")) }).collect())
}

pub fn task_change(tool: &str, input: &Value) -> Option<TaskChange> {
    match tool {
        "TodoWrite" => task_list(&input["todos"]).map(TaskChange::Replace),
        t if t.ends_with("update_plan") => match task_list(&input["items"]) {
            Some(list) => Some(TaskChange::Replace(list)),
            None => {
                let done: Vec<usize> = input["done"].as_array().map(|d| d.iter().filter_map(|n| n.as_u64()).map(|n| n as usize).collect()).unwrap_or_default();
                let (advance, start) = (input["advance"].as_bool().unwrap_or(false), input["start"].as_u64().map(|n| n as usize));
                (advance || !done.is_empty() || start.is_some()).then_some(TaskChange::Moves { advance, done, start })
            }
        },
        _ => None,
    }
}

pub fn apply_tasks(tasks: &mut Vec<Task>, change: TaskChange) {
    match change {
        TaskChange::Replace(list) => *tasks = list,
        TaskChange::Moves { advance, done, start } => {
            let mut set = |n: usize, status| {
                if let Some(t) = n.checked_sub(1).and_then(|i| tasks.get_mut(i)) {
                    t.status = status;
                }
            };
            for n in done {
                set(n, TaskStatus::Done);
            }
            if let Some(n) = start {
                set(n, TaskStatus::Active);
            }
            if advance {
                if let Some(i) = tasks.iter().position(|t| t.status == TaskStatus::Active) {
                    tasks[i].status = TaskStatus::Done;
                }
                if let Some(t) = tasks.iter_mut().find(|t| t.status == TaskStatus::Pending) {
                    t.status = TaskStatus::Active;
                }
            }
        }
    }
}

/// (done, total, the task under way) for "3/7 · Pill and card"; none without a list.
pub fn task_progress(tasks: &[Task]) -> Option<(usize, usize, Option<String>)> {
    if tasks.is_empty() {
        return None;
    }
    let done = tasks.iter().filter(|t| t.status == TaskStatus::Done).count();
    let current = tasks.iter().find(|t| t.status == TaskStatus::Active).map(|t| t.content.clone());
    Some((done, tasks.len(), current))
}

/// The first line of Claude's last answer that says something (headings skipped, list marks and
/// markdown emphasis dropped), at most 140 characters.
pub fn summary_of(answer: &str) -> Option<String> {
    let line = answer.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).find_map(|l| {
        let l = l.trim_start_matches(['-', '*', '>', '•']).trim();
        let l = l.replace("**", "").replace("__", "").replace('`', "");
        (!l.is_empty()).then_some(l)
    })?;
    Some(cut(&line, 140))
}

/// How long a run took: "45 sn", "12 dk", "2 sa 5 dk".
pub fn run_length(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s % 3600 / 60) {
        (0, 0) => trf("{} sn", &[&s]),
        (0, m) => trf("{} dk", &[&m]),
        (h, 0) => trf("{} sa", &[&h]),
        (h, m) => trf("{} sa {} dk", &[&h, &m]),
    }
}

/// The card's line once a run ended: "Bitti · 12 dk · 9 dosya"; an error says only itself.
pub fn done_status(text: &str, run_ms: u64, files: usize) -> String {
    // Made in either language (it may have changed since).
    if text != "Bitti" && text != tr("Bitti") {
        return text.to_string();
    }
    match files {
        0 => trf("Bitti · {}", &[&run_length(run_ms)]),
        n => trf("Bitti · {} · {} dosya", &[&run_length(run_ms), &n]),
    }
}

/// StopFailure's `error` in a few words.
pub fn error_text(error: &str) -> &'static str {
    match error {
        "rate_limit" => tr("Kota doldu"),
        "overloaded" => tr("Sunucu yoğun"),
        "authentication_failed" => tr("Oturum açılmalı"),
        "billing_error" => tr("Ödeme sorunu"),
        "max_output_tokens" => tr("Yanıt sınırı aşıldı"),
        _ => tr("Bir hata oldu"),
    }
}

/// A permission Claude waits for. `id` names the hook process waiting for the card's answer
/// (empty: nothing waits, the card only shows it); `nonce` is the secret an answer must carry,
/// which only the Now Bar learns (the id is a window title anyone can read).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub id: String,
    pub tool: String,
    pub detail: String,
    #[serde(default)]
    pub nonce: String,
    /// The subagent that asked (Claude Code's `agent_id`; empty: the main agent).
    #[serde(default)]
    pub agent: String,
}

/// One tool call, as an ask names it: what PostToolUse or PermissionDenied says ran or was refused.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    pub detail: String,
}

/// What a waiting PermissionRequest hook prints for Claude Code once the card is answered.
pub fn decision_json(allow: bool) -> String {
    let decision = if allow {
        serde_json::json!({"behavior": "allow"})
    } else {
        serde_json::json!({"behavior": "deny", "message": "Now Bar'dan reddedildi"})
    };
    serde_json::json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}}).to_string()
}

/// The card shows this much of a command; longer, or more lines, and only Claude's dialog may
/// answer (the card must never approve what it doesn't show).
const ASK_SHOWN: usize = 120;

fn command_lines(input: &Value) -> Vec<String> {
    text(input, "command").lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// The whole path, the home folder written `~` (a bare "settings.json" could be any).
pub fn tidy_path(path: &str, home: &str) -> String {
    let home = home.trim_end_matches(['\\', '/']);
    match path.get(..home.len()) {
        Some(head) if !home.is_empty() && head.eq_ignore_ascii_case(home) && path[home.len()..].starts_with(['\\', '/']) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// Whether the card may answer this permission: only when it shows all of what is asked (a
/// file's path, not its new text: that is Claude's dialog's diff). A question (it wants
/// answers) and a plan (never shown) are Claude's own dialog's.
pub fn card_can_answer(tool: &str, input: &Value) -> bool {
    let whole = whole_detail(tool, input);
    let shown = !whole.is_empty() && whole.chars().count() <= ASK_SHOWN;
    shown
        && match tool {
            "Bash" | "PowerShell" | "Monitor" => command_lines(input).len() == 1,
            "Edit" | "Write" | "Read" | "NotebookEdit" | "WebFetch" | "WebSearch" | "Skill" | "Grep" | "Glob" => true,
            _ => false,
        }
}

/// What the permission is for: the whole command (or its first line and how many more), the
/// file's path, the address, the search; a question's text.
pub fn ask_detail(tool: &str, input: &Value) -> String {
    cut(&whole_detail(tool, input), ASK_SHOWN)
}

fn whole_detail(tool: &str, input: &Value) -> String {
    match tool {
        "Bash" | "PowerShell" | "Monitor" => match command_lines(input).as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [first, rest @ ..] => trf("{} (+{} satır)", &[first, &rest.len()]),
        },
        "Edit" | "MultiEdit" | "Write" | "Read" | "NotebookEdit" => {
            let home = std::env::var("USERPROFILE").unwrap_or_default();
            tidy_path(input["file_path"].as_str().or(input["notebook_path"].as_str()).unwrap_or(""), &home)
        }
        "WebFetch" => text(input, "url"),
        "WebSearch" => text(input, "query"),
        "Skill" => text(input, "skill"),
        "Grep" | "Glob" => match input["path"].as_str().filter(|p| !p.is_empty()) {
            Some(path) => format!("{} · {path}", text(input, "pattern")),
            None => text(input, "pattern"),
        },
        "AskUserQuestion" => input["questions"][0]["question"].as_str().unwrap_or(tr("Bir soru")).to_string(),
        "ExitPlanMode" => tr("Plan onayı").to_string(),
        "Agent" | "Task" => text(input, "description"),
        t => match t.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((server, name)) => format!("{server}: {name}"),
            None => t.to_string(),
        },
    }
}

/// One hook call's news for the session (all optional: most calls carry one thing).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeNote {
    #[serde(default)]
    pub step: Option<Step>,
    #[serde(default)]
    pub tasks: Option<TaskChange>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub ask: Option<Ask>,
    /// The window the prompt was typed in (the foreground at UserPromptSubmit).
    #[serde(default)]
    pub window: Option<i64>,
    /// PostToolUse(Failure): this call ran (so a permission asked for it was given).
    #[serde(default)]
    pub tool_done: Option<Call>,
    /// PermissionDenied: this call was refused.
    #[serde(default)]
    pub denied: Option<Call>,
    /// The subagent the hook ran in (`agent_id`; empty: the main agent).
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub new_prompt: bool,
}

/// A session's state on the Now Bar side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeInfo {
    /// The last three, newest first.
    pub steps: Vec<Step>,
    pub tasks: Vec<Task>,
    pub files: BTreeSet<String>,
    pub summary: Option<String>,
    pub ask: Option<Ask>,
    pub window: Option<i64>,
}

impl ClaudeInfo {
    pub fn apply(&mut self, n: &ClaudeNote) {
        if n.new_prompt {
            self.steps.clear();
            self.files.clear();
            self.tasks.clear();
            self.summary = None;
            self.ask = None;
        }
        if let Some(w) = n.window {
            self.window = Some(w);
        }
        let waited_on = self.ask.as_ref().filter(|a| !a.id.is_empty());
        let its = |c: &Option<Call>| waited_on.zip(c.as_ref()).is_some_and(|(a, c)| a.tool == c.tool && a.detail == c.detail && a.agent == n.agent);
        let its_agent_moved_on = waited_on.is_some_and(|a| a.agent == n.agent) && n.step.is_some();
        match &n.ask {
            Some(ask) => self.ask = Some(ask.clone()),
            // An ask a hook waits on goes when its own call ran or was refused, the agent that
            // asked took its next step (answered in Claude's dialog), the turn ended, or the hook
            // gave up (AskGone): a parallel subagent's step or call is not its answer.
            None if waited_on.is_some() => {
                if its(&n.tool_done) || its(&n.denied) || its_agent_moved_on || n.summary.is_some() {
                    self.ask = None;
                }
            }
            // One nothing waits on: Claude moving on means it was answered in its dialog.
            None if n.step.is_some() || n.tasks.is_some() || n.summary.is_some() || n.tool_done.is_some() || n.denied.is_some() => self.ask = None,
            None => {}
        }
        if let Some(s) = &n.step {
            self.steps.insert(0, s.clone());
            self.steps.truncate(3);
        }
        if let Some(c) = &n.tasks {
            apply_tasks(&mut self.tasks, c.clone());
        }
        if let Some(f) = &n.file {
            self.files.insert(f.to_lowercase());
        }
        if let Some(s) = &n.summary {
            self.summary = Some(s.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(tool: &str, input: serde_json::Value) -> String {
        step_label(&step_of(tool, &input))
    }

    #[test]
    fn steps_say_what_and_on_what() {
        assert_eq!(step("Edit", json!({"file_path": r"C:\Users\Ekin\proje\src\app.rs"})), "Düzenliyor: app.rs");
        assert_eq!(step("Write", json!({"file_path": "C:/x/notes.md"})), "Yazıyor: notes.md");
        assert_eq!(step("Read", json!({"file_path": r"C:\x\pill.rs"})), "Okuyor: pill.rs");
        assert_eq!(step("Bash", json!({"command": "cargo test --lib", "description": "Run the tests"})), "Çalıştırıyor: Run the tests");
        assert_eq!(step("PowerShell", json!({"command": "Get-Process nowbar\nmore"})), "Çalıştırıyor: Get-Process nowbar");
        assert_eq!(step("Grep", json!({"pattern": "paint_shelf"})), "Arıyor: paint_shelf");
        assert_eq!(step("Glob", json!({"pattern": "**/*.rs"})), "Arıyor: **/*.rs");
        assert_eq!(step("WebFetch", json!({"url": "https://code.claude.com/docs/en/hooks.md"})), "Okuyor: code.claude.com");
        assert_eq!(step("WebSearch", json!({"query": "rust ole drag"})), "Arıyor: rust ole drag");
        assert_eq!(step("Agent", json!({"description": "Review the shelf branch"})), "Alt ajan: Review the shelf branch");
        assert_eq!(step("mcp__ccd_turn__update_plan", json!({})), "Araç: update_plan");
        assert_eq!(step("Skill", json!({"skill": "brainstorming"})), "Araç: Skill");
    }

    #[test]
    fn long_targets_are_cut() {
        let s = step("Bash", json!({"command": "a".repeat(200)}));
        assert!(s.chars().count() <= 60 && s.ends_with('…'), "{s}");
    }

    #[test]
    fn todo_write_replaces_the_list() {
        let input = json!({"todos": [
            {"content": "Model", "status": "completed", "activeForm": "Modelling"},
            {"content": "Painters", "status": "in_progress", "activeForm": "Painting"},
            {"content": "Wiring", "status": "pending", "activeForm": "Wiring"}
        ]});
        let mut tasks = Vec::new();
        apply_tasks(&mut tasks, task_change("TodoWrite", &input).unwrap());
        assert_eq!(task_progress(&tasks), Some((1, 3, Some("Painters".to_string()))));
    }

    #[test]
    fn update_plan_items_then_advance_done_and_start() {
        let mut tasks = Vec::new();
        let items = json!({"items": [
            {"content": "A", "status": "in_progress"}, {"content": "B", "status": "pending"}, {"content": "C", "status": "pending"}
        ], "scope": "first_task"});
        apply_tasks(&mut tasks, task_change("mcp__ccd_turn__update_plan", &items).unwrap());
        assert_eq!(task_progress(&tasks), Some((0, 3, Some("A".to_string()))));
        apply_tasks(&mut tasks, task_change("mcp__ccd_turn__update_plan", &json!({"advance": true})).unwrap());
        assert_eq!(task_progress(&tasks), Some((1, 3, Some("B".to_string()))));
        apply_tasks(&mut tasks, task_change("mcp__ccd_turn__update_plan", &json!({"done": [2], "start": 3})).unwrap());
        assert_eq!(task_progress(&tasks), Some((2, 3, Some("C".to_string()))));
        apply_tasks(&mut tasks, task_change("mcp__ccd_turn__update_plan", &json!({"advance": true})).unwrap());
        assert_eq!(task_progress(&tasks), Some((3, 3, None)));
        assert!(task_change("Edit", &json!({})).is_none());
        assert_eq!(task_progress(&[]), None);
    }

    #[test]
    fn the_summary_is_the_answers_first_real_line() {
        assert_eq!(summary_of("## Bitti\n\nRaf **artık** `ShareX` ile çalışıyor.\nİkinci satır.").as_deref(), Some("Raf artık ShareX ile çalışıyor."));
        assert_eq!(summary_of("Tek satır").as_deref(), Some("Tek satır"));
        assert_eq!(summary_of("- madde bir\n- madde iki").as_deref(), Some("madde bir"));
        assert_eq!(summary_of("  \n\n"), None);
        assert!(summary_of(&"x".repeat(500)).unwrap().chars().count() <= 140);
    }

    #[test]
    fn errors_in_turkish() {
        assert_eq!(error_text("rate_limit"), "Kota doldu");
        assert_eq!(error_text("overloaded"), "Sunucu yoğun");
        assert_eq!(error_text("authentication_failed"), "Oturum açılmalı");
        assert_eq!(error_text("max_output_tokens"), "Yanıt sınırı aşıldı");
        assert_eq!(error_text("something_new"), "Bir hata oldu");
    }

    #[test]
    fn asks_show_the_command_or_the_file() {
        assert_eq!(ask_detail("Bash", &json!({"command": "cargo build --release\necho done"})), "cargo build --release (+1 satır)", "the lines it hides are counted");
        assert_eq!(ask_detail("WebFetch", &json!({"url": "https://unpkg.com/x.svg"})), "https://unpkg.com/x.svg");
        assert_eq!(ask_detail("mcp__github__create_pr", &json!({})), "github: create_pr");
    }

    fn note() -> ClaudeNote {
        ClaudeNote::default()
    }

    #[test]
    fn info_keeps_three_steps_the_files_and_clears_the_ask() {
        let mut info = ClaudeInfo::default();
        info.apply(&ClaudeNote { new_prompt: true, window: Some(42), ..note() });
        for f in ["a.rs", "b.rs", "a.rs", "c.rs"] {
            let input = json!({ "file_path": format!(r"C:\x\{f}") });
            info.apply(&ClaudeNote { step: Some(step_of("Edit", &input)), file: file_of("Edit", &input), ..note() });
        }
        assert_eq!(info.steps.len(), 3, "the last three");
        assert_eq!(step_label(&info.steps[0]), "Düzenliyor: c.rs", "newest first");
        assert_eq!(info.files.len(), 3, "a.rs counted once");
        assert_eq!(info.window, Some(42));
        info.apply(&ClaudeNote { ask: Some(Ask { tool: "Bash".into(), detail: "ls".into(), ..Default::default() }), ..note() });
        info.apply(&ClaudeNote { step: Some(step_of("Read", &json!({"file_path": "x"}))), ..note() });
        assert!(info.ask.is_none(), "an ask nothing waits on: the next step means it was answered");
        info.apply(&ClaudeNote { summary: Some("Tamam".into()), ..note() });
        assert_eq!(info.summary.as_deref(), Some("Tamam"));
        info.apply(&ClaudeNote { new_prompt: true, window: Some(43), ..note() });
        assert!(info.steps.is_empty() && info.files.is_empty() && info.summary.is_none(), "a new prompt starts a new run");
        assert_eq!(info.window, Some(43));
    }

    #[test]
    fn the_decision_is_claude_codes_permission_output() {
        let allow: Value = serde_json::from_str(&decision_json(true)).unwrap();
        assert_eq!(allow, json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}));
        let deny: Value = serde_json::from_str(&decision_json(false)).unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(deny["hookSpecificOutput"]["decision"]["message"], "Now Bar'dan reddedildi");
    }

    #[test]
    fn run_lengths_read_like_a_person_says_them() {
        assert_eq!(run_length(4_000), "4 sn");
        assert_eq!(run_length(59_400), "59 sn");
        assert_eq!(run_length(60_000), "1 dk");
        assert_eq!(run_length(12 * 60_000 + 40_000), "12 dk");
        assert_eq!(run_length(2 * 3_600_000 + 5 * 60_000), "2 sa 5 dk");
        assert_eq!(run_length(3_600_000), "1 sa");
    }

    #[test]
    fn done_status_says_how_long_and_how_many_files() {
        assert_eq!(done_status("Bitti", 12 * 60_000, 9), "Bitti · 12 dk · 9 dosya");
        assert_eq!(done_status("Bitti", 30_000, 0), "Bitti · 30 sn");
        assert_eq!(done_status("Kota doldu", 30_000, 2), "Kota doldu", "an error says only itself");
    }

    #[test]
    fn the_card_answers_only_what_it_shows_in_full() {
        assert!(card_can_answer("Bash", &json!({"command": "cargo build --release"})));
        assert!(card_can_answer("Monitor", &json!({"command": "tail -f log.txt"})));
        assert!(!card_can_answer("Bash", &json!({"command": "cat > x.sh <<'EOF'\nrm -rf /\nEOF"})), "more lines than it shows");
        assert!(!card_can_answer("Bash", &json!({"command": "x".repeat(200)})), "longer than it shows");
        assert!(card_can_answer("Edit", &json!({"file_path": r"C:\x\src\app.rs"})));
        assert!(card_can_answer("WebFetch", &json!({"url": "https://unpkg.com/x.svg"})));
        assert!(!card_can_answer("AskUserQuestion", &json!({"questions": [{"question": "Hangisi?"}]})), "a question wants answers, not allow");
        assert!(!card_can_answer("ExitPlanMode", &json!({"plan": "..."})), "a plan the card never shows");
        assert!(!card_can_answer("mcp__github__create_pr", &json!({"title": "x"})), "its input isn't shown");
        assert!(!card_can_answer("Agent", &json!({"description": "x", "prompt": "long"})));
    }

    #[test]
    fn asks_show_their_whole_target() {
        assert_eq!(ask_detail("Bash", &json!({"command": "cd build\nmake\nmake install"})), "cd build (+2 satır)");
        assert_eq!(ask_detail("Monitor", &json!({"command": "tail -f log.txt"})), "tail -f log.txt");
        assert_eq!(ask_detail("Grep", &json!({"pattern": "TODO", "path": r"C:\Users\Ekin"})), r"TODO · C:\Users\Ekin");
        assert_eq!(ask_detail("WebSearch", &json!({"query": "rust ole"})), "rust ole");
        assert_eq!(ask_detail("Skill", &json!({"skill": "brainstorming"})), "brainstorming");
        assert_eq!(ask_detail("AskUserQuestion", &json!({"questions": [{"question": "Hangi renk?"}]})), "Hangi renk?");
        assert_eq!(ask_detail("ExitPlanMode", &json!({"plan": "x"})), "Plan onayı");
    }

    #[test]
    fn only_its_own_call_or_its_agent_moving_on_clears_an_ask() {
        let ask = Ask { id: "9-1".into(), tool: "Bash".into(), detail: "ls".into(), nonce: "n".into(), agent: String::new() };
        let call = |tool: &str, detail: &str| Some(Call { tool: tool.into(), detail: detail.into() });
        let mut info = ClaudeInfo::default();
        info.apply(&ClaudeNote { ask: Some(ask.clone()), ..note() });
        info.apply(&ClaudeNote { tool_done: call("Bash", "ls"), agent: "sub-1".into(), ..note() });
        assert!(info.ask.is_some(), "a parallel subagent ran the same command: not this one");
        info.apply(&ClaudeNote { tool_done: call("Bash", "cargo test"), ..note() });
        assert!(info.ask.is_some(), "another Bash call finishing is not it");
        info.apply(&ClaudeNote { step: Some(step_of("Read", &json!({"file_path": "a.rs"}))), agent: "sub-1".into(), ..note() });
        assert!(info.ask.is_some(), "a subagent's step is not the answer");
        info.apply(&ClaudeNote { tool_done: call("Bash", "ls"), ..note() });
        assert!(info.ask.is_none(), "its own call ran: it was allowed");

        info.apply(&ClaudeNote { ask: Some(ask.clone()), ..note() });
        info.apply(&ClaudeNote { step: Some(step_of("Read", &json!({"file_path": "a.rs"}))), ..note() });
        assert!(info.ask.is_none(), "the agent that asked moved on: answered (or refused) in Claude's dialog");

        info.apply(&ClaudeNote { ask: Some(ask), ..note() });
        info.apply(&ClaudeNote { denied: call("Bash", "ls"), ..note() });
        assert!(info.ask.is_none(), "refused");
    }

    #[test]
    fn only_what_shows_whole_is_answerable() {
        assert!(!card_can_answer("Grep", &json!({"pattern": "x".repeat(200)})), "a cut pattern");
        assert!(!card_can_answer("Edit", &json!({"file_path": format!(r"C:\{}\a.rs", "d".repeat(150))})), "a cut path");
        assert_eq!(ask_detail("Edit", &json!({"file_path": r"C:\x\src\app.rs"})), r"C:\x\src\app.rs", "the whole path: settings.json could be any");
        assert_eq!(tidy_path(r"C:\Users\Ekin\.claude\settings.json", r"C:\Users\Ekin"), r"~\.claude\settings.json");
        assert_eq!(tidy_path(r"c:\users\ekin\x", r"C:\Users\Ekin"), r"~\x");
        assert_eq!(tidy_path(r"C:\Users\Ekin\x", r"C:\Users\E"), r"C:\Users\Ekin\x", "not a prefix of a longer name");
    }

    #[test]
    fn a_finished_tool_is_not_a_second_step() {
        let mut info = ClaudeInfo::default();
        info.apply(&ClaudeNote { step: Some(step_of("Read", &json!({"file_path": "a.rs"}))), ..note() });
        info.apply(&ClaudeNote { tool_done: Some(Call { tool: "Read".into(), detail: "a.rs".into() }), ..note() });
        info.apply(&ClaudeNote { step: Some(step_of("Edit", &json!({"file_path": "b.rs"}))), ..note() });
        let labels: Vec<String> = info.steps.iter().map(step_label).collect();
        assert_eq!(labels, ["Düzenliyor: b.rs", "Okuyor: a.rs"]);
    }

    #[test]
    fn a_new_prompt_starts_a_new_task_list() {
        let mut info = ClaudeInfo::default();
        info.apply(&ClaudeNote { tasks: task_change("TodoWrite", &json!({"todos": [{"content": "A", "status": "completed"}]})), ..note() });
        info.apply(&ClaudeNote { new_prompt: true, ..note() });
        assert!(info.tasks.is_empty());
    }

    #[test]
    fn targets_and_lists_are_kept_small() {
        assert!(step_of("Bash", &json!({"command": "y".repeat(5000)})).target.chars().count() <= 120);
        let todos: Vec<serde_json::Value> = (0..100).map(|i| json!({"content": format!("t{i}"), "status": "pending"})).collect();
        let Some(TaskChange::Replace(list)) = task_change("TodoWrite", &json!({ "todos": todos })) else { panic!("a list") };
        assert_eq!(list.len(), 40);
    }

    #[test]
    fn a_reads_file_is_not_a_change() {
        assert_eq!(file_of("Read", &json!({"file_path": "C:/x/a.rs"})), None);
        assert_eq!(file_of("Write", &json!({"file_path": "C:/x/a.rs"})).as_deref(), Some("C:/x/a.rs"));
    }
}
