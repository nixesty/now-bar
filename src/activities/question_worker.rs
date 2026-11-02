//! A question to Claude Code on its own thread: `claude -p` with the question on stdin and the
//! answer read from its stream as it's written. Claude Code's own sign-in and account; no tools,
//! no hooks, no session kept.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::question::{find_claude, needs_login, parse_line, Event, SYSTEM_HINT};
use crate::platform::log::log;

/// An answer that hasn't ended in this long won't.
const GIVE_UP_AFTER: Duration = Duration::from_secs(180);
/// The card repaints at most this often while the answer streams.
const POST_EVERY: Duration = Duration::from_millis(60);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AskState {
    #[default]
    Idle,
    /// Asked; nothing written yet.
    Thinking,
    Writing,
    Done,
    /// No `claude` on this PC.
    NoClaude,
    /// Claude Code isn't signed in (its own `/login`).
    NotSignedIn,
    Failed,
    TimedOut,
}

/// What the card reads: which question (a newer one wins), where it is, the answer so far.
#[derive(Clone, Debug, Default)]
pub struct AskBox {
    pub run: u32,
    pub state: AskState,
    pub answer: String,
}

pub type AskMailbox = Arc<Mutex<AskBox>>;

/// `claude` on PATH or where its installers put it.
pub fn locate() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from).unwrap_or_default();
    find_claude(&std::env::var("PATH").unwrap_or_default(), &var("USERPROFILE"), &var("APPDATA"), Path::is_file)
}

/// The process answering, with the question it answers.
type Slot = Arc<Mutex<Option<(u32, Child)>>>;

/// The slot's process if it answers `run` (another question's stays where it is).
fn take_run(slot: &Slot, run: u32) -> Option<Child> {
    let mut s = slot.lock().ok()?;
    if s.as_ref().is_some_and(|(r, _)| *r == run) { s.take().map(|(_, c)| c) } else { None }
}

/// Ends `child` and what it started: npm's claude.cmd is cmd.exe running node, and a kill of
/// cmd.exe alone leaves node answering.
fn end(mut child: Child) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let taskkill = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into())).join(r"System32\taskkill.exe");
    let tree = Command::new(taskkill)
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !tree.is_ok_and(|s| s.success()) {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[derive(Default)]
pub struct AskWorker {
    run: Arc<AtomicU32>,
    child: Slot,
}

impl AskWorker {
    /// The question asked last: an answer for another is stale.
    pub fn current(&self) -> u32 {
        self.run.load(Ordering::SeqCst)
    }

    /// Asks `question` (one at a time: a new one stops the last); the answer comes into `mailbox`,
    /// with `post` after each piece.
    pub fn ask(&self, question: String, mailbox: AskMailbox, post: fn()) {
        self.stop();
        let run = self.run.fetch_add(1, Ordering::SeqCst) + 1;
        let (current, child_slot) = (self.run.clone(), self.child.clone());
        // Only the question still asked writes to the card, checked under the box's lock: a new
        // question's "Thinking" is never overwritten by the last one's words.
        let say = move |state: AskState, answer: Option<String>| {
            let Ok(mut b) = mailbox.lock() else { return };
            if current.load(Ordering::SeqCst) != run {
                return;
            }
            b.run = run;
            b.state = state;
            if let Some(a) = answer {
                b.answer = a;
            }
            drop(b);
            post();
        };
        say(AskState::Thinking, Some(String::new()));
        let spawned = std::thread::Builder::new().name("question".into()).spawn(move || {
            let Some(exe) = locate() else {
                log("question: no claude on this PC");
                return say(AskState::NoClaude, None);
            };
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            // An empty folder: no project's files or settings come with the question.
            let here = std::env::temp_dir().join("pebbly-question");
            let _ = std::fs::create_dir_all(&here);
            // No tools and no MCP servers (--strict-mcp-config without a config: none start).
            let started = Command::new(&exe)
                .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--tools", "", "--strict-mcp-config", "--no-session-persistence"])
                .args(["--settings", r#"{"disableAllHooks":true}"#, "--append-system-prompt", SYSTEM_HINT])
                // Started from inside a Claude Code session, it would refuse to nest.
                .env_remove("CLAUDECODE")
                .env_remove("CLAUDE_CODE_ENTRYPOINT")
                // Our own hook stays out of it even if hooks run (no live item for the card's question).
                .env("PEBBLY_QUESTION", "1")
                .current_dir(here)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            let mut child = match started {
                Ok(c) => c,
                Err(e) => {
                    log(&format!("question: claude didn't start ({:?})", e.kind()));
                    return say(AskState::NoClaude, None);
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(question.as_bytes());
            }
            let Some(stdout) = child.stdout.take() else { return say(AskState::Failed, None) };
            if let Ok(mut slot) = child_slot.lock() {
                *slot = Some((run, child));
            }
            // A watchdog: an answer that never ends is stopped, and says so.
            let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (watch_slot, watch_timed_out) = (child_slot.clone(), timed_out.clone());
            let _ = std::thread::Builder::new().name("question-watch".into()).spawn(move || {
                std::thread::sleep(GIVE_UP_AFTER);
                if let Some(c) = take_run(&watch_slot, run) {
                    log(&format!("question {run}: no answer in {} s", GIVE_UP_AFTER.as_secs()));
                    watch_timed_out.store(true, Ordering::SeqCst);
                    end(c);
                }
            });
            let (mut answer, mut deltas, mut last_post) = (String::new(), false, Instant::now() - POST_EVERY);
            let mut outcome: Option<AskState> = None;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                match parse_line(&line) {
                    Some(Event::Delta(t)) => {
                        deltas = true;
                        answer.push_str(&t);
                    }
                    Some(Event::Message(t)) if !deltas => answer = t,
                    Some(Event::Message(_)) => continue,
                    Some(Event::Done { text, error }) => {
                        outcome = Some(match (error, needs_login(&text)) {
                            (true, true) => AskState::NotSignedIn,
                            (true, false) => AskState::Failed,
                            (false, _) => {
                                answer = text;
                                AskState::Done
                            }
                        });
                        break;
                    }
                    None => continue,
                }
                if last_post.elapsed() >= POST_EVERY {
                    last_post = Instant::now();
                    say(AskState::Writing, Some(answer.clone()));
                }
            }
            if let Some(mut c) = take_run(&child_slot, run) {
                let _ = c.wait();
            }
            let state = outcome.unwrap_or(if timed_out.load(Ordering::SeqCst) { AskState::TimedOut } else { AskState::Failed });
            if state != AskState::Done {
                log(&format!("question {run}: {state:?}"));
            }
            say(state, (state == AskState::Done).then_some(answer));
        });
        if let Err(e) = spawned {
            log(&format!("question: no thread ({e})"));
        }
    }

    /// The question asked stops (the card closed, or another came); taskkill runs off the UI thread.
    pub fn stop(&self) {
        self.run.fetch_add(1, Ordering::SeqCst);
        if let Some((_, c)) = self.child.lock().ok().and_then(|mut s| s.take()) {
            let _ = std::thread::Builder::new().name("question-stop".into()).spawn(move || end(c));
        }
    }
}
