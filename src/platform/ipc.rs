//! One command from `pebbly.exe push | claude-hook | update` to the running Pebbly: JSON in a
//! WM_COPYDATA to its PebblyMain window (no pipe or port to keep open: the window is there anyway).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA};

use crate::activities::live::LiveMsg;

/// `COPYDATASTRUCT.dwData`: "NB" and format 1; anything else is someone else's message.
pub const TAG: usize = 0x4E42_0001;
pub const MAX_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Command {
    Live(Box<LiveMsg>),
    /// Install the latest release if it's newer (the settings window's row, `pebbly.exe update`).
    Update,
    /// The card's answer to a permission, to the hook process waiting for it (`id`), with the
    /// ask's nonce (proof it comes from Pebbly).
    Answer { id: String, nonce: String, allow: bool },
    /// The hook stopped waiting (Claude's own dialog took over): the card drops its buttons.
    AskGone { id: String },
}

impl Command {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Option<Command> {
        serde_json::from_str(text).ok()
    }
}

/// What Pebbly answers a command with: for a permission a hook waits on (it sent an ask
/// id), the seconds to wait for the card; otherwise 1, taken.
pub fn answer_for(c: &Command, asks_on: bool, wait_s: u32) -> isize {
    match c {
        Command::Live(m) if asks_on && m.claude.as_ref().and_then(|n| n.ask.as_ref()).is_some_and(|a| !a.id.is_empty()) => wait_s.max(2) as isize,
        _ => 1,
    }
}

/// `c` small enough for WM_COPYDATA: a very long task list is dropped first (the rest still
/// says what Claude does).
pub fn fitted(c: Command) -> Command {
    match c {
        Command::Live(mut m) if c_len(&m) > MAX_BYTES => {
            if let Some(n) = m.claude.as_mut() {
                n.tasks = None;
            }
            Command::Live(m)
        }
        c => c,
    }
}

fn c_len(m: &LiveMsg) -> usize {
    serde_json::to_string(m).map_or(0, |s| s.len()) + 32
}

/// Sends `c` to the window of class `class`; false when there is none, it timed out or the
/// command is too big.
pub fn send_to(class: PCWSTR, c: &Command) -> bool {
    send_answering(class, PCWSTR::null(), c).is_some_and(|a| a >= 1)
}

/// Sends `c` to the window of class `class` (and title `title`, unless null); its answer, or
/// none when there is no such window, it timed out or the command is too big.
pub fn send_answering(class: PCWSTR, title: PCWSTR, c: &Command) -> Option<usize> {
    let json = c.to_json();
    if json.len() > MAX_BYTES {
        return None;
    }
    unsafe {
        let Ok(hwnd) = FindWindowW(class, title) else { return None };
        let data = COPYDATASTRUCT { dwData: TAG, cbData: json.len() as u32, lpData: json.as_ptr() as *mut _ };
        // A hung Pebbly must not hold a Claude hook: give up after a second.
        let mut answer = 0usize;
        let sent = SendMessageTimeoutW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&data as *const _ as isize), SMTO_ABORTIFHUNG, 1000, Some(&mut answer));
        (sent.0 != 0 && answer != 0).then_some(answer)
    }
}

/// Sends `c` to the running Pebbly.
pub fn send(c: &Command) -> bool {
    send_to(windows::core::w!("PebblyMain"), c)
}

/// The command in a WM_COPYDATA's `lparam`, if it is ours.
///
/// # Safety
/// `lp` must be the `lparam` of a WM_COPYDATA the window procedure is handling.
pub unsafe fn receive(lp: LPARAM) -> Option<Command> {
    let data = unsafe { (lp.0 as *const COPYDATASTRUCT).as_ref()? };
    if data.dwData != TAG || data.lpData.is_null() || data.cbData as usize > MAX_BYTES {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize) };
    Command::from_json(std::str::from_utf8(bytes).ok()?)
}

static QUEUE: Mutex<Vec<Command>> = Mutex::new(Vec::new());

/// Called by the window procedure; the app takes them on `UiEvent::Ipc`.
pub fn queue(c: Command) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(c);
    }
}

pub fn take() -> Vec<Command> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::live::LiveState;

    #[test]
    fn command_round_trips() {
        assert_eq!(Command::from_json(&Command::Update.to_json()), Some(Command::Update));
        let live = Command::Live(Box::new(LiveMsg { id: "b".into(), title: "çalışma".into(), text: "x".into(), progress: Some(40), state: LiveState::Wait, claude: None, app: None }));
        assert_eq!(Command::from_json(&live.to_json()), Some(live));
        assert_eq!(Command::from_json("{}"), None);
        assert_eq!(Command::from_json("not json"), None);
    }

    #[test]
    fn answers_and_gone_asks_round_trip() {
        for c in [Command::Answer { id: "12-34".into(), nonce: "n".into(), allow: true }, Command::AskGone { id: "12-34".into() }] {
            assert_eq!(Command::from_json(&c.to_json()), Some(c));
        }
    }

    #[test]
    fn a_long_task_list_is_dropped_to_fit() {
        use crate::activities::claude::{ClaudeNote, Task, TaskChange, TaskStatus};
        let tasks = (0..200).map(|i| Task { content: format!("görev {i} ").repeat(6), status: TaskStatus::Pending }).collect();
        let note = ClaudeNote { tasks: Some(TaskChange::Replace(tasks)), summary: Some("özet".into()), ..Default::default() };
        let big = Command::Live(Box::new(LiveMsg { id: "claude:s".into(), title: "x".into(), text: String::new(), progress: None, state: LiveState::Run, claude: Some(note), app: None }));
        assert!(big.to_json().len() > MAX_BYTES);
        let small = fitted(big);
        assert!(small.to_json().len() <= MAX_BYTES);
        let Command::Live(m) = small else { panic!("still a live message") };
        let n = m.claude.unwrap();
        assert!(n.tasks.is_none() && n.summary.as_deref() == Some("özet"), "only the list goes");
        assert_eq!(fitted(Command::Update), Command::Update);
    }

    #[test]
    fn an_ask_is_answered_with_the_wait() {
        use crate::activities::claude::{Ask, ClaudeNote};
        let ask = |id: &str| {
            let note = ClaudeNote { ask: Some(Ask { id: id.into(), tool: "Bash".into(), detail: "ls".into(), ..Default::default() }), ..Default::default() };
            Command::Live(Box::new(LiveMsg { id: "claude:s".into(), title: "x".into(), text: String::new(), progress: None, state: LiveState::Wait, claude: Some(note), app: None }))
        };
        assert_eq!(answer_for(&ask("9-1"), true, 60), 60, "the hook waits this many seconds");
        assert_eq!(answer_for(&ask("9-1"), false, 60), 1, "asks off: taken, no wait");
        assert_eq!(answer_for(&ask(""), true, 60), 1, "an async hook has no id: nothing waits");
        assert_eq!(answer_for(&Command::Update, true, 60), 1);
    }

    #[test]
    fn send_without_a_pebbly_is_false_and_quick() {
        let t = std::time::Instant::now();
        assert!(!send_to(windows::core::w!("PebblyNoSuchWindow"), &Command::Update));
        assert!(t.elapsed() < std::time::Duration::from_millis(200));
    }

    #[test]
    fn a_receive_of_someone_elses_copydata_is_none() {
        let json = Command::Update.to_json();
        let mut data = COPYDATASTRUCT { dwData: TAG, cbData: json.len() as u32, lpData: json.as_ptr() as *mut _ };
        assert_eq!(unsafe { receive(LPARAM(&data as *const _ as isize)) }, Some(Command::Update));
        data.dwData = 7;
        assert_eq!(unsafe { receive(LPARAM(&data as *const _ as isize)) }, None, "another tag");
        data.dwData = TAG;
        data.cbData = MAX_BYTES as u32 + 1;
        assert_eq!(unsafe { receive(LPARAM(&data as *const _ as isize)) }, None, "too big");
    }
}
