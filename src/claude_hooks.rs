//! Claude Code's hooks for Pebbly in `~/.claude/settings.json`: `pebbly.exe claude-hooks
//! install | remove`, and the exe's path kept current at start (as autostart follows the exe).
//! The pure part works on the settings' JSON; ours are the hooks whose first arg is `claude-hook`.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

/// The events Pebbly listens to (PermissionRequest waits for the card's answer).
pub const EVENTS: [&str; 10] = ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionDenied", "Notification", "Stop", "StopFailure", "SessionEnd"];

/// A hook entry of ours.
pub fn is_ours(hook: &Value) -> bool {
    hook["args"].get(0).and_then(Value::as_str) == Some("claude-hook")
}

fn our_hook(event: &str, exe: &str) -> Value {
    if event == "PermissionRequest" {
        // Synchronous: it may answer; 320 s outlasts the longest wait (300 s).
        json!({"type": "command", "command": exe, "args": ["claude-hook", "--ask"], "timeout": 320})
    } else {
        json!({"type": "command", "command": exe, "args": ["claude-hook"], "async": true})
    }
}

/// `settings` without our hooks; events left with no hooks go.
pub fn remove(mut settings: Value) -> Value {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else { return settings };
    for entries in hooks.values_mut() {
        let Some(list) = entries.as_array_mut() else { continue };
        // Only the entries we emptied go: another tool's odd shapes stay as they are.
        list.retain_mut(|entry| {
            let Some(inner) = entry.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
            let had = inner.len();
            inner.retain(|h| !is_ours(h));
            !(inner.is_empty() && had > 0)
        });
    }
    hooks.retain(|_, entries| entries.as_array().is_none_or(|l| !l.is_empty()));
    settings
}

/// `settings` with our hooks for every event, pointing at `exe` (any old ones replaced); `None`
/// when its `hooks` isn't an object (nothing we could add to).
pub fn install(settings: Value, exe: &str) -> Option<Value> {
    let mut settings = remove(settings);
    if !settings.is_object() {
        settings = Value::Object(Map::new());
    }
    let hooks = settings.as_object_mut()?.entry("hooks").or_insert_with(|| json!({})).as_object_mut()?;
    for event in EVENTS {
        let entries = hooks.entry(event).or_insert_with(|| json!([]));
        if let Some(list) = entries.as_array_mut() {
            list.push(json!({"matcher": "", "hooks": [our_hook(event, exe)]}));
        }
    }
    Some(settings)
}

/// Our hooks repointed at `exe` when the exe they point at is gone (Pebbly was moved): the
/// events, matchers and timeouts stay as the user left them. `None` when there are none, they
/// point here, or their exe still exists (a dev build running from elsewhere must not take them
/// over).
pub fn follow(mut settings: Value, exe: &str, exists: impl Fn(&str) -> bool) -> Option<Value> {
    let ours: Vec<&mut Value> = settings
        .get_mut("hooks")
        .and_then(Value::as_object_mut)?
        .values_mut()
        .filter_map(Value::as_array_mut)
        .flatten()
        .filter_map(|entry| entry.get_mut("hooks").and_then(Value::as_array_mut))
        .flatten()
        .filter(|h| is_ours(h))
        .collect();
    let commands: Vec<&str> = ours.iter().filter_map(|h| h["command"].as_str()).collect();
    if commands.is_empty() || commands.iter().all(|c| *c == exe) || commands.iter().any(|c| *c != exe && exists(c)) {
        return None;
    }
    for h in ours {
        h["command"] = json!(exe);
    }
    Some(settings)
}

/// The settings file's text: `{}` when it doesn't exist yet. Any other failure (locked by Claude
/// Code or an antivirus, not UTF-8) is an error: reading it as empty would overwrite it.
pub fn read_settings(path: &std::path::Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("{}".into()),
        other => other,
    }
}

/// Claude Code's user settings: `%CLAUDE_CONFIG_DIR%\settings.json`, else `~\.claude\settings.json`.
pub fn settings_path() -> Option<PathBuf> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".claude")))?;
    Some(dir.join("settings.json"))
}

/// Reads, changes and writes Claude's settings (backups first); `Ok(false)` when nothing changed.
pub fn rewrite(change: impl FnOnce(Value) -> Option<Value>) -> std::io::Result<bool> {
    let path = settings_path().ok_or_else(|| std::io::Error::other("no user profile"))?;
    rewrite_at(&path, change)
}

/// `rewrite` for the settings file at `path` (the setup's sandbox has its own).
pub fn rewrite_at(path: &std::path::Path, change: impl FnOnce(Value) -> Option<Value>) -> std::io::Result<bool> {
    let path = path.to_path_buf();
    let text = read_settings(&path)?;
    let current: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(std::io::Error::other)?;
    let Some(next) = change(current) else { return Ok(false) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let backup = path.with_extension("json.before-pebbly");
    // The first is the file before Pebbly ever touched it; `.pebbly-last` is the one before this write.
    if path.exists() {
        std::fs::copy(&path, if backup.exists() { path.with_extension("json.pebbly-last") } else { backup })?;
    }
    let pretty = serde_json::to_string_pretty(&next).map_err(std::io::Error::other)?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, if text.contains("\r\n") { pretty.replace('\n', "\r\n") } else { pretty })?;
    std::fs::rename(&tmp, &path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXE: &str = r"C:\Apps\Pebbly\pebbly.exe";

    fn ours(v: &Value, event: &str) -> Vec<Value> {
        v["hooks"][event].as_array().into_iter().flatten().flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default()).filter(is_ours).collect()
    }

    #[test]
    fn install_adds_every_event_with_the_ask_synchronous() {
        let v = install(json!({}), EXE).unwrap();
        for event in EVENTS {
            let hooks = ours(&v, event);
            assert_eq!(hooks.len(), 1, "{event}");
            assert_eq!(hooks[0]["command"], EXE);
        }
        let ask = &ours(&v, "PermissionRequest")[0];
        assert_eq!(ask["args"], json!(["claude-hook", "--ask"]));
        assert_eq!(ask["timeout"], 320);
        assert!(ask.get("async").is_none(), "an async hook can't decide");
        assert_eq!(ours(&v, "Stop")[0]["async"], true, "the rest never hold Claude up");
    }

    #[test]
    fn install_twice_is_once_and_moves_the_path() {
        let v = install(install(json!({}), r"C:\old\pebbly.exe").unwrap(), EXE).unwrap();
        assert_eq!(ours(&v, "PreToolUse").len(), 1);
        assert_eq!(ours(&v, "PreToolUse")[0]["command"], EXE);
    }

    #[test]
    fn other_hooks_and_settings_stay() {
        let orca = json!({"type": "command", "command": "orca-hook.cmd", "timeout": 10});
        let before = json!({"model": "opus", "hooks": {"Stop": [{"hooks": [orca.clone()]}]}});
        let v = install(before, EXE).unwrap();
        assert_eq!(v["model"], "opus");
        assert!(v["hooks"]["Stop"].to_string().contains("orca-hook.cmd"));
        let gone = remove(v);
        assert_eq!(ours(&gone, "Stop").len(), 0);
        assert!(gone["hooks"]["Stop"].to_string().contains("orca-hook.cmd"), "Orca's stays");
        assert!(gone["hooks"].get("PreToolUse").is_none(), "an event left empty goes");
    }

    #[test]
    fn the_users_key_order_stays() {
        let before: Value = serde_json::from_str(r#"{"permissions": {}, "model": "opus", "env": {}}"#).unwrap();
        let after = serde_json::to_string(&install(before, EXE).unwrap()).unwrap();
        let at = |k: &str| after.find(k).unwrap();
        assert!(at("permissions") < at("model") && at("model") < at("env") && at("env") < at("\"hooks\""));
    }

    #[test]
    fn remove_leaves_entries_it_cant_read() {
        let odd = json!({"hooks": {"Stop": [{"matcher": "", "command": "legacy.cmd"}]}});
        assert_eq!(remove(odd.clone()), odd, "not ours: kept as it is");
    }

    #[test]
    fn install_refuses_a_hooks_that_isnt_an_object() {
        assert_eq!(install(json!({"hooks": "off"}), EXE), None);
        assert!(install(json!({}), EXE).is_some());
    }

    #[test]
    fn follow_only_repoints_what_is_there() {
        let mut v = install(json!({}), r"C:\old\pebbly.exe").unwrap();
        v["hooks"].as_object_mut().unwrap().remove("Notification");
        v["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"] = json!(100);
        let moved = follow(v, EXE, |_| false).unwrap();
        assert!(moved["hooks"].get("Notification").is_none(), "an event the user dropped stays dropped");
        assert_eq!(ours(&moved, "PermissionRequest")[0]["timeout"], 100, "the user's timeout stays");
        assert_eq!(ours(&moved, "PermissionRequest")[0]["command"], EXE);
    }

    #[test]
    fn only_a_missing_file_reads_as_empty() {
        let dir = std::env::temp_dir().join(format!("pebbly-hooks-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(read_settings(&dir.join("absent.json")).unwrap(), "{}");
        assert!(read_settings(&dir).is_err(), "unreadable (locked, a folder) is an error, not an empty file to overwrite");
    }

    #[test]
    fn follow_moves_only_existing_hooks() {
        let gone = |_: &str| false;
        assert_eq!(follow(json!({}), EXE, gone), None, "never installed: nothing to move");
        let v = install(json!({}), r"C:\old\pebbly.exe").unwrap();
        assert_eq!(follow(v.clone(), EXE, |_| true), None, "the other exe is still there (a dev build ran): leave them");
        let moved = follow(v, EXE, gone).expect("their exe is gone: the app was moved");
        assert_eq!(ours(&moved, "Stop")[0]["command"], EXE);
        assert_eq!(follow(moved, EXE, gone), None, "already here");
    }
}
