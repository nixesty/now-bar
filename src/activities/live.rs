//! Live activities other programs send (`pebbly.exe push`, Claude Code's hooks): the messages
//! and their state. The pure part; platform/ipc.rs carries a message to the running Pebbly.

use serde::{Deserialize, Serialize};

use crate::i18n::{tr, trf};

use crate::activities::api::Button;
use crate::activities::claude::{ask_detail, error_text, file_of, step_label, step_of, summary_of, task_change, Ask, Call, ClaudeInfo, ClaudeNote};
use crate::render::crest::Crest;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiveState {
    Run,
    Wait,
    Done,
    Clear,
}

/// One message: what an item (by `id`) is doing now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveMsg {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub progress: Option<u8>,
    pub state: LiveState,
    /// Claude Code's details (the step, the tasks, the ask); none for `push`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<ClaudeNote>,
    /// An app's, through the API (never over WM_COPYDATA).
    #[serde(skip)]
    pub app: Option<Box<AppMeta>>,
}

/// How an app's item shows its icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppIcon {
    Glyph(char),
    /// The picture as decoded (128 px); `id` tells pictures apart for the painters' cache.
    Image { id: u64, src: Crest },
}

/// What the API adds to an item: who sent it and how it looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMeta {
    /// The caller's key (`api::Caller::key`): the app the user allowed, or is asked about.
    pub owner: String,
    pub name: String,
    /// The exe's full path.
    pub exe: String,
    /// The app's own id for the item, as its events say it.
    pub client_id: String,
    pub icon: Option<AppIcon>,
    pub accent: Option<(u8, u8, u8)>,
    pub buttons: Vec<Button>,
    /// A permission prompt, not something the app sent.
    pub prompt: bool,
    /// A notification's or a prompt's time on screen, from each message.
    pub life_ms: Option<u64>,
}

impl AppMeta {
    /// "Ekin · ekin.exe": the name it gave and where it really runs from.
    pub fn sender(&self) -> String {
        format!("{} · {}", self.name, self.exe.rsplit(['\\', '/']).next().unwrap_or(&self.exe))
    }
}

/// Whether `push` (and anything over WM_COPYDATA) may use `id`: the API's ids are the apps'.
pub fn push_id_ok(id: &str) -> bool {
    !id.starts_with("app:") && !id.starts_with("ask:")
}

/// `push`'s flags (the arguments after `push`); `None` when a flag is unknown, a value is
/// missing or wrong, or `--id` / `--title` isn't there.
pub fn parse_push(args: &[String]) -> Option<LiveMsg> {
    let (mut id, mut title, mut text, mut progress, mut state) = (None, None, String::new(), None, LiveState::Run);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next()?;
        match flag.as_str() {
            "--id" => id = Some(value.clone()),
            "--title" => title = Some(value.clone()),
            "--text" => text = value.clone(),
            "--progress" => progress = Some(value.parse::<u32>().ok()?.min(100) as u8),
            "--state" => state = serde_json::from_value(serde_json::Value::String(value.clone())).ok()?,
            _ => return None,
        }
    }
    Some(LiveMsg { id: id?, title: title?, text, progress, state, claude: None, app: None })
}

/// A wait is a moment (ahead of the user's pick) this long, then it stays ahead of the music.
pub const LIVE_WAIT_URGENT_MS: u64 = 4_000;
/// A finished item shows this long, then leaves.
pub const LIVE_DONE_MS: u64 = 5_000;
/// No message for this long: the sender is gone.
pub const LIVE_STALE_MS: u64 = 3_600_000;
/// A Claude item still working or waiting with no message for this long: Esc (at a prompt or at
/// a permission) sends no `Stop`.
pub const LIVE_CLAUDE_STALE_MS: u64 = 900_000;
pub const LIVE_MAX: usize = 8;

/// The stack's key for an id (FNV-1a).
pub fn live_key(id: &str) -> u32 {
    id.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// `s` at most `n` characters: cut ones end with "…" (by characters: Turkish letters are two bytes).
pub(crate) fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n - 1).chain(['…']).collect()
}

/// One item: the latest message and when things happened (GetTickCount64 ms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveItem {
    pub key: u32,
    pub id: String,
    pub title: String,
    pub text: String,
    pub progress: Option<u8>,
    pub state: LiveState,
    /// Sent by Claude Code's hooks (the id starts with `claude:`).
    pub claude: bool,
    /// When this run began: the first message, or Run after Done.
    pub started_ms: u64,
    /// The last change of state (what the stack ranks by).
    pub changed_ms: u64,
    /// The last message, of any state.
    pub seen_ms: u64,
    /// A Claude session's steps, tasks, files, answer and ask.
    pub info: ClaudeInfo,
    /// When the current ask arrived.
    pub asked_ms: u64,
    /// Sent by an app through the API.
    pub app: Option<Box<AppMeta>>,
    /// A notification's or a prompt's end (GetTickCount64 ms).
    pub until_ms: Option<u64>,
}

const ASK_SETTLE_MS: u64 = 500;

#[derive(Clone, Debug, Default)]
pub struct LiveSet {
    items: Vec<LiveItem>,
}

impl LiveSet {
    pub fn apply(&mut self, m: LiveMsg, now_ms: u64) {
        let id = cut(&m.id, 64);
        let key = live_key(&id);
        // Another id on the same 32-bit key: the item stays its sender's.
        if self.items.iter().any(|i| i.key == key && i.id != id) {
            return;
        }
        if m.state == LiveState::Clear {
            self.remove(key);
            return;
        }
        let (title, text) = (cut(&m.title, 40), cut(&m.text, 120));
        let until_ms = m.app.as_ref().and_then(|a| a.life_ms).map(|life| now_ms + life);
        if let Some(i) = self.items.iter_mut().find(|i| i.key == key) {
            if m.app.is_some() {
                // New buttons count from now: a click aimed at the old ones lands on nothing.
                if i.app.as_ref().map(|a| &a.buttons) != m.app.as_ref().map(|a| &a.buttons) {
                    i.asked_ms = now_ms;
                }
                (i.app, i.until_ms) = (m.app.clone(), until_ms);
            }
            let old_ask = i.info.ask.as_ref().map(|a| a.id.clone());
            if let Some(n) = &m.claude {
                if n.new_prompt {
                    i.started_ms = now_ms;
                }
                i.info.apply(n);
            }
            // A permission a hook still waits on keeps the item waiting, with its text and its
            // place, whatever a parallel subagent ran meanwhile.
            let held = m.state == LiveState::Run && i.info.ask.as_ref().is_some_and(|a| !a.id.is_empty());
            let (state, text) = if held { (LiveState::Wait, std::mem::take(&mut i.text)) } else { (m.state, text) };
            if i.state != state {
                // A run after done is the next prompt: its time counts from zero.
                if state == LiveState::Run && i.state == LiveState::Done {
                    i.started_ms = now_ms;
                }
                i.changed_ms = now_ms;
            }
            (i.title, i.text, i.progress, i.state, i.seen_ms) = (title, text, m.progress, state, now_ms);
            // A new ask is news even while already waiting: it comes forward again.
            if m.claude.as_ref().and_then(|n| n.ask.as_ref()).is_some_and(|a| Some(&a.id) != old_ask.as_ref()) {
                (i.changed_ms, i.asked_ms) = (now_ms, now_ms);
            }
            return;
        }
        // One app can't fill the stack: its own quietest makes room.
        if let Some(owner) = m.app.as_ref().map(|a| a.owner.as_str()) {
            let mine = self.items.iter().filter(|i| i.app.as_ref().is_some_and(|a| a.owner == owner));
            if mine.clone().count() >= crate::activities::api::PER_APP_MAX {
                if let Some(quietest) = mine.min_by_key(|i| i.seen_ms).map(|i| i.key) {
                    self.remove(quietest);
                }
            }
        }
        if self.items.len() >= LIVE_MAX {
            if let Some(quietest) = self.items.iter().min_by_key(|i| i.seen_ms).map(|i| i.key) {
                self.remove(quietest);
            }
        }
        let claude = id.starts_with("claude:");
        let mut info = ClaudeInfo::default();
        if let Some(n) = &m.claude {
            info.apply(n);
        }
        self.items.push(LiveItem {
            key,
            id,
            title,
            text,
            progress: m.progress,
            state: m.state,
            claude,
            started_ms: now_ms,
            changed_ms: now_ms,
            seen_ms: now_ms,
            info,
            asked_ms: now_ms,
            app: m.app,
            until_ms,
        });
    }

    /// The first ms `i` is gone (GetTickCount64): done when the stack drops it (age 5 s, as
    /// `rank`), stale once a full quiet period has passed.
    fn gone_at(i: &LiveItem) -> u64 {
        let stale = match i.state {
            LiveState::Done => i.changed_ms + LIVE_DONE_MS,
            LiveState::Run | LiveState::Wait if i.claude => i.seen_ms + LIVE_CLAUDE_STALE_MS + 1,
            _ => i.seen_ms + LIVE_STALE_MS + 1,
        };
        i.until_ms.map_or(stale, |until| until.min(stale))
    }

    /// Drops what is over or stale (but not `open`, the item whose card is open: its answer is
    /// being read); whether anything left.
    pub fn expire(&mut self, now_ms: u64, open: Option<u32>) -> bool {
        let before = self.items.len();
        self.items.retain(|i| Some(i.key) == open || now_ms < Self::gone_at(i));
        self.items.len() != before
    }

    /// In how many ms a moment ends: a wait's 4 s in front, a done item's 5 s, a notification's
    /// time (the tick's job).
    pub fn moment_in(&self, now_ms: u64) -> Option<u64> {
        let end = |i: &LiveItem| match i.state {
            LiveState::Wait => Some(i.changed_ms + LIVE_WAIT_URGENT_MS),
            LiveState::Done => Some(Self::gone_at(i)),
            LiveState::Run | LiveState::Clear => None,
        };
        Self::soonest(self.items.iter().flat_map(|i| [end(i), i.until_ms]).flatten(), now_ms)
    }

    /// The keys of one app's items (`owner`: its caller's key).
    pub fn owned_by(&self, owner: &str) -> Vec<u32> {
        self.items.iter().filter(|i| i.app.as_ref().is_some_and(|a| a.owner == owner)).map(|i| i.key).collect()
    }

    /// In how many ms a quiet item leaves (minutes away: a long timer's job, not the tick's).
    pub fn leave_in(&self, now_ms: u64) -> Option<u64> {
        Self::soonest(self.items.iter().filter(|i| i.state != LiveState::Done).map(Self::gone_at), now_ms)
    }

    fn soonest(at: impl Iterator<Item = u64>, now_ms: u64) -> Option<u64> {
        at.filter(|at| *at > now_ms).min().map(|at| at - now_ms)
    }

    /// The permission id a card's buttons answer (none without a waiting hook).
    pub fn ask_id(&self, key: u32) -> Option<String> {
        self.get(key).and_then(|i| i.info.ask.as_ref()).map(|a| a.id.clone()).filter(|id| !id.is_empty())
    }

    /// Whether `key`'s ask has been on screen long enough for a click to be meant for it (a
    /// click landing just as a new ask is painted was aimed at the one before).
    pub fn ask_settled(&self, key: u32, now_ms: u64) -> bool {
        self.get(key).is_some_and(|i| now_ms >= i.asked_ms + ASK_SETTLE_MS)
    }

    /// Whether an app's item has been on screen (with these buttons) long enough for a click
    /// to be meant for it.
    pub fn app_settled(&self, key: u32, now_ms: u64) -> bool {
        self.get(key).is_some_and(|i| now_ms >= i.asked_ms + crate::activities::api::SETTLE_MS)
    }

    /// The card answered: Claude carries on (or was told no), so the item runs again.
    pub fn answered(&mut self, key: u32, allow: bool, now_ms: u64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.key == key) {
            i.info.ask = None;
            i.text = if allow { tr("İzin verildi") } else { tr("Reddedildi") }.to_string();
            if i.state != LiveState::Run {
                (i.state, i.changed_ms) = (LiveState::Run, now_ms);
            }
            i.seen_ms = now_ms;
        }
    }

    /// The ids of the asks a hook waits on.
    pub fn waiting_asks(&self) -> Vec<String> {
        self.items.iter().filter_map(|i| i.info.ask.as_ref()).filter(|a| !a.id.is_empty()).map(|a| a.id.clone()).collect()
    }

    /// The hook stopped waiting: the ask stays in Claude's own dialog, not on the card.
    pub fn drop_ask(&mut self, id: &str) {
        for i in &mut self.items {
            if i.info.ask.as_ref().is_some_and(|a| a.id == id) {
                i.info.ask = None;
            }
        }
    }

    pub fn remove(&mut self, key: u32) {
        self.items.retain(|i| i.key != key);
    }

    pub fn get(&self, key: u32) -> Option<&LiveItem> {
        self.items.iter().find(|i| i.key == key)
    }

    pub fn items(&self) -> &[LiveItem] {
        &self.items
    }
}

/// What one Claude Code hook call means for the session's item (`claude:<session_id>`, titled
/// with the project folder); `None` for events that change nothing.
///
/// The hook process adds `pebbly_window` (the foreground window at UserPromptSubmit) and
/// `pebbly_ask_id` (when it waits for an answer from the card) to Claude Code's JSON.
pub fn claude_msg(hook: &serde_json::Value) -> Option<LiveMsg> {
    let session = hook["session_id"].as_str().filter(|s| !s.is_empty())?;
    let (tool, input) = (hook["tool_name"].as_str().unwrap_or(""), &hook["tool_input"]);
    let agent = hook["agent_id"].as_str().unwrap_or("").to_string();
    let mut note = ClaudeNote { agent: agent.clone(), ..Default::default() };
    let call = || Some(Call { tool: tool.to_string(), detail: ask_detail(tool, input) });
    let (state, text) = match hook["hook_event_name"].as_str()? {
        "UserPromptSubmit" => {
            note.new_prompt = true;
            note.window = hook["pebbly_window"].as_i64();
            (LiveState::Run, tr("Düşünüyor").to_string())
        }
        // Only PreToolUse moves the task list and counts the file: a move must not apply twice.
        "PreToolUse" => {
            let step = step_of(tool, input);
            (note.tasks, note.file) = (task_change(tool, input), file_of(tool, input));
            let label = step_label(&step);
            note.step = Some(step);
            (LiveState::Run, label)
        }
        // The tool ran: no second step (PreToolUse said it), but a permission asked for it was given.
        "PostToolUse" | "PostToolUseFailure" => {
            note.tool_done = call();
            (LiveState::Run, step_label(&step_of(tool, input)))
        }
        "PermissionDenied" => {
            note.denied = call();
            (LiveState::Run, tr("Reddedildi").to_string())
        }
        "PermissionRequest" => {
            let (id, nonce) = (hook["pebbly_ask_id"].as_str().unwrap_or(""), hook["pebbly_ask_nonce"].as_str().unwrap_or(""));
            note.ask = Some(Ask { id: id.to_string(), tool: tool.to_string(), detail: ask_detail(tool, input), nonce: nonce.to_string(), agent });
            match tool {
                "" => (LiveState::Wait, tr("İzin istiyor").to_string()),
                "AskUserQuestion" => (LiveState::Wait, tr("Soru soruyor").to_string()),
                "ExitPlanMode" => (LiveState::Wait, tr("Plan onayı bekliyor").to_string()),
                tool => (LiveState::Wait, trf("İzin istiyor: {}", &[&tool])),
            }
        }
        // permission_prompt comes ~6 s after PermissionRequest (and without the tool), idle_prompt a
        // minute after Stop: only an MCP server's question is news here.
        "Notification" if hook["notification_type"] == "elicitation_dialog" => (LiveState::Wait, tr("Yanıtını bekliyor").to_string()),
        "Stop" => {
            note.summary = summary_of(hook["last_assistant_message"].as_str().unwrap_or(""));
            (LiveState::Done, tr("Bitti").to_string())
        }
        "StopFailure" => (LiveState::Done, error_text(hook["error"].as_str().unwrap_or("")).to_string()),
        "SessionEnd" => (LiveState::Clear, String::new()),
        _ => return None,
    };
    let folder = hook["cwd"].as_str().unwrap_or("").split(['\\', '/']).rfind(|p| !p.is_empty());
    let title = folder.unwrap_or("Claude").to_string();
    let claude = (note != ClaudeNote { agent: note.agent.clone(), ..Default::default() }).then_some(note);
    Some(LiveMsg { id: format!("claude:{session}"), title, text, progress: None, state, claude, app: None })
}

/// The pill's right side: the time this run has taken (or its progress), what it waits for, or
/// "Bitti".
pub fn pill_status(item: &LiveItem, now_ms: u64) -> String {
    match item.state {
        LiveState::Run => match item.progress {
            Some(p) => crate::i18n::percent(p),
            None => {
                let s = now_ms.saturating_sub(item.started_ms) / 1000;
                match s / 3600 {
                    0 => format!("{}:{:02}", s / 60, s % 60),
                    h => format!("{h}:{:02}:{:02}", s % 3600 / 60, s % 60),
                }
            }
        },
        // "İzin istiyor: Bash" → "İzin istiyor": the tool's name is the card's.
        LiveState::Wait => match item.text.split(':').next().map(str::trim) {
            Some(short) if !short.is_empty() => short.to_string(),
            _ => tr("Bekliyor").to_string(),
        },
        LiveState::Done | LiveState::Clear => tr("Bitti").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, state: LiveState) -> LiveMsg {
        LiveMsg { id: id.into(), title: "pebbly".into(), text: String::new(), progress: None, state, claude: None, app: None }
    }

    /// An app's item (`owner` stands for its caller's key) living `life` ms, if a notification.
    fn app_msg(owner: &str, id: &str, life: Option<u64>) -> LiveMsg {
        let meta = AppMeta { owner: owner.into(), name: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into(), client_id: id.into(), icon: None, accent: None, buttons: Vec::new(), prompt: false, life_ms: life };
        LiveMsg { app: Some(Box::new(meta)), ..msg(&format!("app:{owner}:{id}"), LiveState::Wait) }
    }

    #[test]
    fn a_notification_leaves_at_its_end() {
        let mut set = LiveSet::default();
        set.apply(app_msg("o1", "n1", Some(5_000)), 1_000);
        assert_eq!(set.get(live_key("app:o1:n1")).and_then(|i| i.until_ms), Some(6_000));
        assert_eq!(set.moment_in(2_000), Some(3_000), "its wait's moment ends first");
        assert_eq!(set.moment_in(5_500), Some(500), "then it leaves");
        assert!(!set.expire(5_999, None));
        assert!(set.expire(6_000, None));
        assert!(set.items().is_empty());
    }

    #[test]
    fn an_update_moves_the_end() {
        let mut set = LiveSet::default();
        set.apply(app_msg("o1", "n1", Some(5_000)), 1_000);
        set.apply(app_msg("o1", "n1", Some(5_000)), 4_000);
        assert_eq!(set.get(live_key("app:o1:n1")).and_then(|i| i.until_ms), Some(9_000));
        set.apply(app_msg("o1", "a1", None), 4_000);
        assert_eq!(set.get(live_key("app:o1:a1")).and_then(|i| i.until_ms), None, "an activity has no end of its own");
        assert_eq!(set.get(live_key("app:o1:a1")).and_then(|i| i.app.as_ref()).map(|a| a.sender()).as_deref(), Some("Ekin · ekin.exe"));
    }

    #[test]
    fn three_items_per_app_the_quietest_goes() {
        let mut set = LiveSet::default();
        for (n, at) in [("a", 10), ("b", 20), ("c", 30)] {
            set.apply(app_msg("o1", n, None), at);
        }
        set.apply(app_msg("o2", "x", None), 5);
        set.apply(app_msg("o1", "d", None), 40);
        let ids: Vec<&str> = set.items().iter().map(|i| i.id.as_str()).collect();
        assert!(!ids.contains(&"app:o1:a") && ids.contains(&"app:o1:d"), "{ids:?}");
        assert!(ids.contains(&"app:o2:x"), "another app's stays, though quieter");
        set.apply(app_msg("o1", "b", None), 50);
        assert_eq!(set.owned_by("o1").len(), 3, "an update isn't a new item");
    }

    #[test]
    fn push_ids_cannot_take_app_ids() {
        assert!(push_id_ok("yedek") && push_id_ok("claude:s1"));
        assert!(!push_id_ok("app:0123:yedek") && !push_id_ok("ask:0123"));
    }

    #[test]
    fn owned_by_lists_one_apps_items() {
        let mut set = LiveSet::default();
        set.apply(app_msg("o1", "a", None), 0);
        set.apply(app_msg("o2", "b", None), 0);
        set.apply(msg("script", LiveState::Run), 0);
        assert_eq!(set.owned_by("o1"), vec![live_key("app:o1:a")]);
    }

    #[test]
    fn a_key_collision_is_refused() {
        // Two ids on one 32-bit key: the second can't change, or clear, the first's item.
        let mut seen = std::collections::HashMap::new();
        let (a, b) = (0u32..).map(|n| format!("app:o1:{n}")).find_map(|id| seen.insert(live_key(&id), id.clone()).map(|old| (old, id))).unwrap();
        let mut set = LiveSet::default();
        set.apply(LiveMsg { title: "ilk".into(), ..msg(&a, LiveState::Run) }, 0);
        set.apply(LiveMsg { title: "ikinci".into(), ..msg(&b, LiveState::Run) }, 10);
        set.apply(msg(&b, LiveState::Clear), 20);
        assert_eq!(set.items().len(), 1);
        assert_eq!((set.items()[0].id.as_str(), set.items()[0].title.as_str()), (a.as_str(), "ilk"));
    }

    #[test]
    fn apply_adds_updates_and_clears() {
        let mut set = LiveSet::default();
        set.apply(msg("b", LiveState::Run), 1_000);
        assert_eq!(set.items().len(), 1);
        let key = live_key("b");
        assert_eq!(set.get(key).map(|i| (i.started_ms, i.changed_ms, i.state)), Some((1_000, 1_000, LiveState::Run)));
        set.apply(LiveMsg { text: "İzin istiyor: Bash".into(), ..msg("b", LiveState::Wait) }, 3_000);
        let i = set.get(key).unwrap();
        assert_eq!((i.state, i.started_ms, i.changed_ms, i.text.as_str()), (LiveState::Wait, 1_000, 3_000, "İzin istiyor: Bash"));
        set.apply(msg("b", LiveState::Done), 4_000);
        set.apply(msg("b", LiveState::Run), 9_000);
        assert_eq!(set.get(key).map(|i| i.started_ms), Some(9_000), "a new run after done counts from zero");
        set.apply(msg("b", LiveState::Clear), 10_000);
        assert!(set.items().is_empty());
        set.apply(msg("claude:s1", LiveState::Run), 0);
        assert!(set.get(live_key("claude:s1")).unwrap().claude);
        assert_ne!(live_key("a"), live_key("b"));
    }

    #[test]
    fn claude_events_map_to_states() {
        let hook = |extra: serde_json::Value| {
            let mut v = serde_json::json!({"session_id": "s1", "cwd": "C:\\Users\\Ekin\\Desktop\\pebbly", "transcript_path": "x"});
            for (k, val) in extra.as_object().unwrap() {
                v[k] = val.clone();
            }
            claude_msg(&v)
        };
        let event = |name: &str| hook(serde_json::json!({"hook_event_name": name}));
        let text = |m: Option<LiveMsg>| m.map(|m| (m.state, m.text));
        let note = |m: Option<LiveMsg>| m.and_then(|m| m.claude).unwrap_or_default();

        let prompt = hook(serde_json::json!({"hook_event_name": "UserPromptSubmit", "pebbly_window": 4242}));
        assert_eq!(prompt.as_ref().map(|m| (m.id.as_str(), m.title.as_str())), Some(("claude:s1", "pebbly")));
        assert_eq!(text(prompt.clone()), Some((LiveState::Run, "Düşünüyor".into())));
        assert_eq!(note(prompt), ClaudeNote { new_prompt: true, window: Some(4242), ..Default::default() });

        let edit = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Edit", "tool_input": {"file_path": "C:\\x\\app.rs"}});
        assert_eq!(text(hook(edit.clone())), Some((LiveState::Run, "Düzenliyor: app.rs".into())));
        let n = note(hook(edit));
        assert!(n.step.is_some() && n.file.as_deref() == Some("C:\\x\\app.rs"));
        let plan = hook(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "TodoWrite", "tool_input": {"todos": [{"content": "A", "status": "in_progress"}]}}));
        assert!(note(plan).tasks.is_some());
        let after = hook(serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "TodoWrite", "tool_input": {"todos": [{"content": "A", "status": "in_progress"}]}}));
        assert_eq!(text(after.clone()).map(|t| t.0), Some(LiveState::Run));
        assert!(note(after.clone()).tasks.is_none() && note(after.clone()).step.is_none(), "only PreToolUse moves the list and adds a step");
        assert_eq!(note(after).tool_done.map(|c| c.tool).as_deref(), Some("TodoWrite"));
        let failed = hook(serde_json::json!({"hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_input": {"command": "cargo test"}, "agent_id": "a1"}));
        let n = note(failed);
        assert_eq!((n.tool_done, n.agent.as_str()), (Some(Call { tool: "Bash".into(), detail: "cargo test".into() }), "a1"), "it ran (and failed): a permission was given");
        let denied = hook(serde_json::json!({"hook_event_name": "PermissionDenied", "tool_name": "Bash", "tool_input": {"command": "rm x"}}));
        assert_eq!(note(denied).denied, Some(Call { tool: "Bash".into(), detail: "rm x".into() }));

        let ask = hook(serde_json::json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "cargo build"}, "pebbly_ask_id": "77", "pebbly_ask_nonce": "s3cr3t"}));
        assert_eq!(note(ask).ask, Some(Ask { id: "77".into(), tool: "Bash".into(), detail: "cargo build".into(), nonce: "s3cr3t".into(), agent: String::new() }));
        let question = hook(serde_json::json!({"hook_event_name": "PermissionRequest", "tool_name": "AskUserQuestion", "tool_input": {"questions": [{"question": "Hangisi?"}]}}));
        assert_eq!(text(question), Some((LiveState::Wait, "Soru soruyor".into())));
        let stop = hook(serde_json::json!({"hook_event_name": "Stop", "last_assistant_message": "## Özet\nRaf hazır."}));
        assert_eq!(note(stop).summary.as_deref(), Some("Raf hazır."));
        assert_eq!(text(hook(serde_json::json!({"hook_event_name": "StopFailure", "error": "rate_limit"}))), Some((LiveState::Done, "Kota doldu".into())));
        assert_eq!(text(hook(serde_json::json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash"}))), Some((LiveState::Wait, "İzin istiyor: Bash".into())));
        assert_eq!(text(event("PermissionRequest")), Some((LiveState::Wait, "İzin istiyor".into())));
        let notice = |kind: &str| hook(serde_json::json!({"hook_event_name": "Notification", "notification_type": kind, "message": "Claude needs your permission"}));
        assert_eq!(text(notice("elicitation_dialog")), Some((LiveState::Wait, "Yanıtını bekliyor".into())));
        assert_eq!(notice("permission_prompt"), None, "PermissionRequest said it 6 s earlier, with the tool");
        assert_eq!(notice("idle_prompt"), None, "a minute after Stop: it would bring the pill back");
        assert_eq!(notice("auth_success"), None);
        assert_eq!(text(event("Stop")), Some((LiveState::Done, "Bitti".into())));
        assert_eq!(text(event("StopFailure")), Some((LiveState::Done, "Bir hata oldu".into())));
        assert_eq!(event("SessionEnd").map(|m| m.state), Some(LiveState::Clear));
        assert_eq!(event("SessionStart"), None);
        assert_eq!(event("Whatever"), None);
        assert_eq!(claude_msg(&serde_json::json!({"hook_event_name": "Stop", "cwd": "C:\\x"})), None, "no session");
    }

    #[test]
    fn a_claude_item_keeps_its_sessions_info() {
        let mut set = LiveSet::default();
        let step = ClaudeNote { step: Some(crate::activities::claude::step_of("Read", &serde_json::json!({"file_path": "a.rs"}))), ..Default::default() };
        set.apply(LiveMsg { claude: Some(step), ..msg("claude:s1", LiveState::Run) }, 0);
        set.apply(LiveMsg { claude: Some(ClaudeNote { summary: Some("Bitti".into()), ..Default::default() }), ..msg("claude:s1", LiveState::Done) }, 10);
        let info = &set.get(live_key("claude:s1")).unwrap().info;
        assert_eq!((info.steps.len(), info.summary.as_deref()), (1, Some("Bitti")));
    }

    #[test]
    fn an_answered_or_dropped_ask_leaves_the_card() {
        let mut set = LiveSet::default();
        let ask = |id: &str| ClaudeNote { ask: Some(Ask { id: id.into(), tool: "Bash".into(), detail: "ls".into(), ..Default::default() }), ..Default::default() };
        set.apply(LiveMsg { claude: Some(ask("1-1")), ..msg("claude:a", LiveState::Wait) }, 0);
        set.apply(LiveMsg { claude: Some(ask("2-2")), ..msg("claude:b", LiveState::Wait) }, 0);
        assert_eq!(set.ask_id(live_key("claude:a")).as_deref(), Some("1-1"));
        set.answered(live_key("claude:a"), true, 50);
        let a = set.get(live_key("claude:a")).unwrap();
        assert_eq!((a.info.ask.is_none(), a.state, a.text.as_str()), (true, LiveState::Run, "İzin verildi"));
        set.answered(live_key("claude:b"), false, 60);
        assert_eq!(set.get(live_key("claude:b")).unwrap().text, "Reddedildi");
        set.apply(LiveMsg { claude: Some(ask("3-3")), ..msg("claude:a", LiveState::Wait) }, 70);
        set.drop_ask("9-9");
        assert!(set.get(live_key("claude:a")).unwrap().info.ask.is_some(), "another id: not this one");
        set.drop_ask("3-3");
        let a = set.get(live_key("claude:a")).unwrap();
        assert_eq!((a.info.ask.is_none(), a.state), (true, LiveState::Wait), "still waiting, in Claude's own dialog");
    }

    #[test]
    fn a_waiting_ask_keeps_the_item_waiting_and_a_new_one_comes_forward() {
        let mut set = LiveSet::default();
        let ask = |id: &str| ClaudeNote { ask: Some(Ask { id: id.into(), tool: "Bash".into(), detail: "ls".into(), nonce: "n".into(), agent: String::new() }), ..Default::default() };
        let step = ClaudeNote { step: Some(crate::activities::claude::step_of("Read", &serde_json::json!({"file_path": "a.rs"}))), agent: "sub-1".into(), ..Default::default() };
        set.apply(LiveMsg { claude: Some(ask("1-1")), text: "İzin istiyor: Bash".into(), ..msg("claude:a", LiveState::Wait) }, 100);
        set.apply(LiveMsg { claude: Some(step), text: "Okuyor: a.rs".into(), ..msg("claude:a", LiveState::Run) }, 200);
        let a = set.get(live_key("claude:a")).unwrap();
        assert_eq!((a.state, a.info.ask.is_some()), (LiveState::Wait, true), "a subagent's step doesn't hide the ask");
        assert_eq!((a.text.as_str(), a.changed_ms), ("İzin istiyor: Bash", 100), "nor says its step in the wait, nor comes forward again");
        set.apply(LiveMsg { claude: Some(ask("2-2")), ..msg("claude:a", LiveState::Wait) }, 300);
        assert_eq!(set.get(live_key("claude:a")).unwrap().changed_ms, 300, "a new ask is news: it comes forward again");
    }

    #[test]
    fn a_fresh_ask_isnt_answered_by_a_click_meant_for_the_last() {
        let mut set = LiveSet::default();
        let ask = |id: &str| ClaudeNote { ask: Some(Ask { id: id.into(), tool: "Bash".into(), detail: "ls".into(), ..Default::default() }), ..Default::default() };
        let key = live_key("claude:a");
        set.apply(LiveMsg { claude: Some(ask("1-1")), ..msg("claude:a", LiveState::Wait) }, 1_000);
        assert!(!set.ask_settled(key, 1_300), "on screen for a moment: the click was aimed at what was there before");
        assert!(set.ask_settled(key, 1_600));
        set.apply(LiveMsg { claude: Some(ask("2-2")), ..msg("claude:a", LiveState::Wait) }, 2_000);
        assert!(!set.ask_settled(key, 2_100), "the next one starts over");
    }

    #[test]
    fn a_new_prompt_counts_from_zero() {
        let mut set = LiveSet::default();
        set.apply(msg("claude:a", LiveState::Run), 1_000);
        let prompt = ClaudeNote { new_prompt: true, ..Default::default() };
        set.apply(LiveMsg { claude: Some(prompt), ..msg("claude:a", LiveState::Run) }, 9_000);
        assert_eq!(set.get(live_key("claude:a")).unwrap().started_ms, 9_000, "after an Esc'd run, a new prompt is a new run");
    }

    #[test]
    fn an_item_whose_card_is_open_stays() {
        let mut set = LiveSet::default();
        set.apply(msg("claude:a", LiveState::Done), 0);
        assert!(!set.expire(LIVE_DONE_MS + 1, Some(live_key("claude:a"))), "its card is open: it waits for the card to close");
        assert!(set.expire(LIVE_DONE_MS + 1, None));
    }

    #[test]
    fn claude_title_is_the_project_folder() {
        let title = |cwd: &str| claude_msg(&serde_json::json!({"session_id": "s", "hook_event_name": "Stop", "cwd": cwd})).map(|m| m.title);
        assert_eq!(title("/home/k/proj/").as_deref(), Some("proj"));
        assert_eq!(title("C:\\Users\\Ekin\\Masaüstü\\çalışma").as_deref(), Some("çalışma"));
        assert_eq!(title("").as_deref(), Some("Claude"));
    }

    #[test]
    fn pill_status_by_state() {
        let item = |state: LiveState, text: &str, progress: Option<u8>| {
            let mut set = LiveSet::default();
            set.apply(LiveMsg { text: text.into(), progress, ..msg("b", state) }, 0);
            set.items()[0].clone()
        };
        assert_eq!(pill_status(&item(LiveState::Run, "", None), 252_000), "4:12");
        assert_eq!(pill_status(&item(LiveState::Run, "", None), 3_725_000), "1:02:05");
        assert_eq!(pill_status(&item(LiveState::Run, "", Some(40)), 252_000), "%40");
        assert_eq!(pill_status(&item(LiveState::Wait, "İzin istiyor: Bash", None), 0), "İzin istiyor");
        assert_eq!(pill_status(&item(LiveState::Wait, "Yanıtını bekliyor", None), 0), "Yanıtını bekliyor");
        assert_eq!(pill_status(&item(LiveState::Wait, "", None), 0), "Bekliyor");
        assert_eq!(pill_status(&item(LiveState::Done, "Bitti", None), 0), "Bitti");
    }

    #[test]
    fn repeating_a_state_doesnt_reorder() {
        let mut set = LiveSet::default();
        set.apply(msg("b", LiveState::Run), 1_000);
        set.apply(msg("b", LiveState::Run), 9_000);
        let i = set.get(live_key("b")).unwrap();
        assert_eq!((i.changed_ms, i.seen_ms), (1_000, 9_000));
    }

    #[test]
    fn stale_items_expire_after_an_hour() {
        let mut set = LiveSet::default();
        set.apply(msg("b", LiveState::Run), 0);
        assert!(!set.expire(LIVE_STALE_MS, None));
        assert!(set.expire(LIVE_STALE_MS + 1, None));
        assert!(set.items().is_empty());
        set.apply(msg("d", LiveState::Done), 0);
        assert!(!set.expire(LIVE_DONE_MS - 1, None));
        assert!(set.expire(LIVE_DONE_MS, None), "done is a moment, gone when the stack drops it");
        set.apply(msg("claude:s1", LiveState::Run), 0);
        set.apply(msg("claude:s2", LiveState::Wait), 0);
        set.apply(msg("script", LiveState::Wait), 0);
        // Esc at a prompt or at a permission ends the turn without a Stop: neither may linger.
        assert!(set.expire(LIVE_CLAUDE_STALE_MS + 1, None), "a Claude run or wait interrupted with Esc");
        assert_eq!(set.items().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["script"], "a script's wait stays");
        assert!(set.expire(LIVE_STALE_MS + 1, None));
    }

    #[test]
    fn moments_end_soon_and_quiet_items_leave_late() {
        let mut set = LiveSet::default();
        assert_eq!((set.moment_in(0), set.leave_in(0)), (None, None));
        // A run has no moment: only its quiet hour, on a long timer (no 1 Hz polling for it).
        set.apply(msg("r", LiveState::Run), 0);
        assert_eq!(set.moment_in(1_000), None);
        assert_eq!(set.leave_in(1_000), Some(LIVE_STALE_MS + 1 - 1_000));
        set.apply(msg("w", LiveState::Wait), 1_000);
        assert_eq!(set.moment_in(2_000), Some(LIVE_WAIT_URGENT_MS - 1_000), "the wait's moment ends");
        set.apply(msg("d", LiveState::Done), 2_500);
        assert_eq!(set.moment_in(3_000), Some(2_000), "the wait's moment, before the done's end");
        assert_eq!(set.moment_in(6_000), Some(1_500), "the done leaves");
    }

    #[test]
    fn at_most_eight_the_quietest_goes() {
        let mut set = LiveSet::default();
        for n in 0..9u64 {
            set.apply(msg(&format!("i{n}"), LiveState::Run), 1_000 + n);
        }
        assert_eq!(set.items().len(), LIVE_MAX);
        assert!(set.get(live_key("i0")).is_none(), "the oldest message went");
        assert!(set.get(live_key("i8")).is_some());
    }

    #[test]
    fn limits_cut_by_characters() {
        let mut set = LiveSet::default();
        let long_id: String = "x".repeat(70);
        set.apply(LiveMsg { title: "ü".repeat(50), text: "ş".repeat(130), ..msg(&long_id, LiveState::Run) }, 0);
        let i = &set.items()[0];
        assert_eq!(i.title.chars().count(), 40);
        assert!(i.title.ends_with('…'));
        assert_eq!(i.text.chars().count(), 120);
        assert_eq!(i.id.chars().count(), 64);
        assert_eq!(i.key, live_key(&i.id));
        set.apply(LiveMsg { title: "Masaüstü".into(), ..msg("short", LiveState::Run) }, 0);
        assert_eq!(set.get(live_key("short")).map(|i| i.title.as_str()), Some("Masaüstü"), "short ones stay whole");
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn push_reads_flags() {
        let m = parse_push(&args(&["--id", "b", "--title", "cargo build", "--progress", "40"]));
        assert_eq!(m, Some(LiveMsg { id: "b".into(), title: "cargo build".into(), text: String::new(), progress: Some(40), state: LiveState::Run, claude: None, app: None }));
        let state = |s: &str| parse_push(&args(&["--id", "b", "--title", "t", "--state", s])).map(|m| m.state);
        assert_eq!(state("wait"), Some(LiveState::Wait));
        assert_eq!(state("done"), Some(LiveState::Done));
        assert_eq!(state("clear"), Some(LiveState::Clear));
        assert_eq!(state("run"), Some(LiveState::Run));
        let text = parse_push(&args(&["--title", "t", "--text", "derleniyor", "--id", "b"])).map(|m| m.text);
        assert_eq!(text.as_deref(), Some("derleniyor"), "any order");
        let progress = parse_push(&args(&["--id", "b", "--title", "t", "--progress", "150"])).and_then(|m| m.progress);
        assert_eq!(progress, Some(100));
    }

    #[test]
    fn push_needs_id_and_title() {
        assert_eq!(parse_push(&args(&["--title", "t"])), None);
        assert_eq!(parse_push(&args(&["--id", "b"])), None);
        assert_eq!(parse_push(&args(&["--id", "b", "--title", "t", "--colour", "red"])), None, "unknown flag");
        assert_eq!(parse_push(&args(&["--id", "b", "--title", "t", "--state", "nap"])), None);
        assert_eq!(parse_push(&args(&["--id", "b", "--title", "t", "--progress", "lots"])), None);
        assert_eq!(parse_push(&args(&["--id", "b", "--title"])), None, "a flag without its value");
    }
}
