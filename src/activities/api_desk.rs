//! The UI thread's side of the notifications API: an app's request becomes an item, a prompt or
//! a refusal; clicks and dismissals go back to the request waiting for them, else to the queue
//! `/v1/events` reads. Pure: the app saves the answers `answer` and `mute` return, and opens the
//! URIs `click` returns.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use crate::activities::api::{
    access, events_json, item_id, ok_json, pending_json, prompt_id, Access, ApiCall, ApiWhat, AppRequest, Caller, Event, EventKind, Events, Kind, Reject, Reply,
    COOLDOWN_MS, PROMPT_MS,
};
use crate::activities::live::{live_key, AppIcon, AppMeta, LiveMsg, LiveSet, LiveState};
use crate::settings::ApiApp;

/// The open prompt: whose it is, its item, and the app's latest message waiting on the answer.
struct Prompt {
    caller: Caller,
    key: u32,
    held: Option<Held>,
}

struct Held {
    req: Box<AppRequest>,
    icon: Option<AppIcon>,
    reply: Sender<Reply>,
}

/// A request waiting (`wait`) for a click on its item, until `until_ms` (its connection gives up
/// then: an item updated with a `wait` each time mustn't pile them up).
struct Waiter {
    key: u32,
    client_id: String,
    reply: Sender<Reply>,
    until_ms: u64,
}

/// A waiter is let go this long after its `wait`: the connection's own clock runs a little behind.
const WAITER_SLACK_MS: u64 = 1_000;

#[derive(Default)]
pub struct Desk {
    prompt: Option<Prompt>,
    /// Exes (lower case) whose prompt was denied or ignored, until when (ms).
    cooldown: HashMap<String, u64>,
    waiters: Vec<Waiter>,
    events: Events,
    /// `/v1/events` calls waiting for something to happen, by caller key.
    listeners: Vec<(String, Sender<Reply>)>,
}

impl Desk {
    /// One request from the server; `apps` are the user's answers, `on` whether Pebbly is
    /// switched on.
    pub fn take(&mut self, call: ApiCall, live: &mut LiveSet, apps: &[ApiApp], on: bool, now: u64) {
        let ApiCall { caller, what, reply } = call;
        if !on {
            let _ = reply.send(Reply::reject(Reject::Disabled));
            return;
        }
        let asking = self.prompt.as_ref().map(|p| &p.caller);
        let cooldown = self.cooldown.get(&caller.exe.to_lowercase()).copied();
        let verdict = access(&caller, apps, asking, cooldown, now);
        match what {
            ApiWhat::Events { wait_s } => {
                if verdict == Access::Denied {
                    let _ = reply.send(Reply::reject(Reject::Denied));
                    return;
                }
                let key = caller.key();
                let events = self.events.take(&key, now);
                if !events.is_empty() || wait_s == 0 || verdict != Access::Allowed {
                    let _ = reply.send(Reply::ok(events_json(&events)));
                    return;
                }
                // One listener per app: the one before hears that nothing came.
                if let Some(i) = self.listeners.iter().position(|(k, _)| *k == key) {
                    let _ = self.listeners.remove(i).1.send(Reply::ok(events_json(&[])));
                }
                let _ = reply.send(Reply::Shown);
                self.listeners.push((key, reply));
            }
            ApiWhat::Show { req, icon, exe_icon } => {
                let clear = matches!(req.kind, Kind::Activity { state: LiveState::Clear, .. });
                let held = Held { req, icon, reply };
                match verdict {
                    Access::Allowed => self.show(&caller, held, live, now),
                    Access::Denied => {
                        let _ = held.reply.send(Reply::reject(Reject::Denied));
                    }
                    // An app the user hasn't allowed has nothing on screen to clear.
                    _ if clear => {
                        let _ = held.reply.send(Reply::ok(ok_json(held.req.id.as_deref().unwrap_or(""), None)));
                    }
                    Access::Busy => {
                        let _ = held.reply.send(Reply::reject(Reject::PermissionBusy));
                    }
                    Access::Cooldown => {
                        let _ = held.reply.send(Reply::reject(Reject::PermissionCooldown));
                    }
                    Access::Ask => match self.prompt.as_mut() {
                        // Its prompt is up: the newer message waits instead.
                        Some(p) => {
                            if let Some(old) = p.held.replace(held) {
                                let _ = old.reply.send(Reply::Done { status: 202, body: pending_json(), retry_after: None });
                            }
                        }
                        None => {
                            let msg = prompt_msg(&caller, exe_icon);
                            let key = live_key(&msg.id);
                            live.apply(msg, now);
                            self.prompt = Some(Prompt { caller, key, held: Some(held) });
                        }
                    },
                }
            }
        }
    }

    fn show(&mut self, caller: &Caller, held: Held, live: &mut LiveSet, now: u64) {
        let Held { req, icon, reply } = held;
        let id = req.id.clone().unwrap_or_default();
        let msg = item_msg(caller, &req, icon);
        let key = live_key(&msg.id);
        let clear = msg.state == LiveState::Clear;
        live.apply(msg, now);
        if clear {
            self.release(key, None);
        }
        if req.wait_s > 0 && !clear && live.get(key).is_some() {
            let _ = reply.send(Reply::Shown);
            self.waiters.push(Waiter { key, client_id: id, reply, until_ms: now + req.wait_s as u64 * 1000 + WAITER_SLACK_MS });
        } else {
            let _ = reply.send(Reply::ok(ok_json(&id, None)));
        }
    }

    /// The prompt painted as `painted` was answered: the answer to keep (none when the click
    /// came too soon or was meant for something else).
    pub fn answer(&mut self, painted: u32, allow: bool, live: &mut LiveSet, now: u64) -> Option<ApiApp> {
        if self.prompt.as_ref().is_none_or(|p| p.key != painted) || !live.app_settled(painted, now) {
            return None;
        }
        let p = self.prompt.take()?;
        live.remove(p.key);
        if allow {
            if let Some(held) = p.held {
                self.show(&p.caller, held, live, now);
            }
        } else {
            if let Some(held) = p.held {
                let _ = held.reply.send(Reply::reject(Reject::Denied));
            }
            self.cooldown.insert(p.caller.exe.to_lowercase(), now + COOLDOWN_MS);
        }
        Some(ApiApp { name: p.caller.app, exe: p.caller.exe, allowed: allow })
    }

    /// Button `i` on `key`'s card: the URI to open, if it has one. A notification goes once
    /// answered; an activity stays (its app decides what happens).
    pub fn click(&mut self, key: u32, i: u8, live: &mut LiveSet, now: u64) -> Option<String> {
        let app = live.get(key).and_then(|item| item.app.clone()).filter(|a| !a.prompt)?;
        if !live.app_settled(key, now) {
            return None;
        }
        let button = app.buttons.get(i as usize)?.clone();
        self.deliver(key, &app.owner, Event { id: app.client_id.clone(), kind: EventKind::Button(button.id), at_ms: now });
        if app.life_ms.is_some() {
            live.remove(key);
        }
        button.uri
    }

    /// `key`'s card was closed with its ×: its app hears it (the app removes the item).
    pub fn dismissed(&mut self, key: u32, live: &mut LiveSet, now: u64) {
        if let Some(app) = live.get(key).and_then(|item| item.app.clone()).filter(|a| !a.prompt) {
            self.deliver(key, &app.owner, Event { id: app.client_id.clone(), kind: EventKind::Dismissed, at_ms: now });
        }
    }

    /// "Sustur" on `key`'s card: the answer to keep; the app's items go.
    pub fn mute(&mut self, key: u32, live: &mut LiveSet) -> Option<ApiApp> {
        let app = live.get(key).and_then(|item| item.app.clone()).filter(|a| !a.prompt)?;
        for k in live.owned_by(&app.owner) {
            live.remove(k);
            for w in self.take_waiters(k) {
                let _ = w.reply.send(Reply::reject(Reject::Denied));
            }
        }
        self.listeners.retain(|(k, _)| *k != app.owner);
        Some(ApiApp { name: app.name.clone(), exe: app.exe.clone(), allowed: false })
    }

    /// After the live set expired or lost items: a prompt gone unanswered cools its exe down,
    /// requests waiting on items that left hear that nothing was clicked.
    pub fn expire(&mut self, live: &mut LiveSet, now: u64) {
        if let Some(p) = self.prompt.take_if(|p| live.get(p.key).is_none()) {
            if let Some(held) = p.held {
                let _ = held.reply.send(Reply::Done { status: 202, body: pending_json(), retry_after: None });
            }
            self.cooldown.insert(p.caller.exe.to_lowercase(), now + COOLDOWN_MS);
        }
        let gone: Vec<u32> = self.waiters.iter().map(|w| w.key).filter(|k| live.get(*k).is_none()).collect();
        for key in gone {
            self.release(key, None);
        }
        self.waiters.retain(|w| now < w.until_ms);
        self.cooldown.retain(|_, until| now < *until);
    }

    /// The API was turned off: whatever waits hears so, and the prompt goes.
    pub fn close(&mut self, live: &mut LiveSet) {
        if let Some(p) = self.prompt.take() {
            live.remove(p.key);
            if let Some(held) = p.held {
                let _ = held.reply.send(Reply::reject(Reject::Disabled));
            }
        }
        for w in std::mem::take(&mut self.waiters) {
            let _ = w.reply.send(Reply::reject(Reject::Disabled));
        }
        for (_, l) in std::mem::take(&mut self.listeners) {
            let _ = l.send(Reply::reject(Reject::Disabled));
        }
    }

    /// The prompt's item, if one is up.
    pub fn prompt_key(&self) -> Option<u32> {
        self.prompt.as_ref().map(|p| p.key)
    }

    /// How many requests wait for a click.
    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.waiters.len()
    }

    /// To the request waiting on `key`, else to the app's `/v1/events` listener, else to its queue.
    fn deliver(&mut self, key: u32, owner: &str, e: Event) {
        let mut heard = false;
        for w in self.take_waiters(key) {
            heard |= w.reply.send(Reply::ok(ok_json(&w.client_id, Some(Some(&e))))).is_ok();
        }
        if heard {
            return;
        }
        if let Some(i) = self.listeners.iter().position(|(k, _)| k == owner) {
            if self.listeners.remove(i).1.send(Reply::ok(events_json(std::slice::from_ref(&e)))).is_ok() {
                return;
            }
        }
        self.events.push(owner, e);
    }

    /// The requests waiting on `key` hear `event` (none: nothing was clicked).
    fn release(&mut self, key: u32, event: Option<&Event>) {
        for w in self.take_waiters(key) {
            let _ = w.reply.send(Reply::ok(ok_json(&w.client_id, Some(event))));
        }
    }

    fn take_waiters(&mut self, key: u32) -> Vec<Waiter> {
        let (mine, rest) = std::mem::take(&mut self.waiters).into_iter().partition(|w| w.key == key);
        self.waiters = rest;
        mine
    }
}

/// An app's item as the live set keeps it: a notification waits for the user (it leads the
/// stack while it is on) and ends; an activity has the state it was sent with.
fn item_msg(caller: &Caller, req: &AppRequest, icon: Option<AppIcon>) -> LiveMsg {
    let id = req.id.clone().unwrap_or_default();
    let (state, progress, life_ms) = match req.kind {
        Kind::Notify { life_ms } => (LiveState::Wait, None, Some(life_ms)),
        Kind::Activity { state, progress } => (state, progress, None),
    };
    let meta = AppMeta { owner: caller.key(), name: caller.app.clone(), exe: caller.exe.clone(), client_id: id.clone(), icon, accent: req.accent, buttons: req.buttons.clone(), prompt: false, life_ms };
    LiveMsg { id: item_id(caller, &id), title: req.title.clone(), text: req.text.clone(), progress, state, claude: None, app: Some(Box::new(meta)) }
}

/// The question whether `caller` may show anything, with its exe's own icon.
fn prompt_msg(caller: &Caller, exe_icon: Option<AppIcon>) -> LiveMsg {
    let meta = AppMeta {
        owner: caller.key(),
        name: caller.app.clone(),
        exe: caller.exe.clone(),
        client_id: String::new(),
        icon: exe_icon,
        accent: None,
        buttons: Vec::new(),
        prompt: true,
        life_ms: Some(PROMPT_MS),
    };
    LiveMsg { id: prompt_id(caller), title: caller.app.clone(), text: String::new(), progress: None, state: LiveState::Wait, claude: None, app: Some(Box::new(meta)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::api::parse_notify;
    use std::sync::mpsc::{channel, Receiver};

    fn ekin() -> Caller {
        Caller { app: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into() }
    }

    fn deniz() -> Caller {
        Caller { app: "Deniz".into(), exe: r"C:\Apps\deniz.exe".into() }
    }

    fn allowed(callers: &[Caller]) -> Vec<ApiApp> {
        callers.iter().map(|c| ApiApp { name: c.app.clone(), exe: c.exe.clone(), allowed: true }).collect()
    }

    fn call(caller: &Caller, json: &str) -> (ApiCall, Receiver<Reply>) {
        let (tx, rx) = channel();
        let mut req = parse_notify(json.as_bytes()).unwrap();
        req.id.get_or_insert_with(|| "n1".into());
        (ApiCall { caller: caller.clone(), what: ApiWhat::Show { req: Box::new(req), icon: None, exe_icon: None }, reply: tx }, rx)
    }

    fn events_call(caller: &Caller, wait_s: u32) -> (ApiCall, Receiver<Reply>) {
        let (tx, rx) = channel();
        (ApiCall { caller: caller.clone(), what: ApiWhat::Events { wait_s }, reply: tx }, rx)
    }

    /// The reply so far: `(0, "shown")` for `Shown`.
    fn heard(rx: &Receiver<Reply>) -> Option<(u16, String)> {
        match rx.try_recv() {
            Ok(Reply::Done { status, body, .. }) => Some((status, body)),
            Ok(Reply::Shown) => Some((0, "shown".into())),
            Err(_) => None,
        }
    }

    #[test]
    fn an_allowed_app_shows_at_once() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"Yedek bitti","text":"12 dosya"}"#);
        desk.take(c, &mut live, &allowed(&[ekin()]), true, 1_000);
        let (status, body) = heard(&rx).unwrap();
        assert_eq!((status, body.contains(r#""id":"n1""#)), (200, true), "{body}");
        let item = &live.items()[0];
        assert_eq!((item.title.as_str(), item.text.as_str(), item.until_ms), ("Yedek bitti", "12 dosya", Some(6_000)));
        assert_eq!(item.app.as_ref().map(|a| (a.client_id.as_str(), a.prompt, a.owner.as_str())), Some(("n1", false, ekin().key().as_str())));
    }

    #[test]
    fn an_unknown_app_is_asked_first() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"Merhaba"}"#);
        desk.take(c, &mut live, &[], true, 1_000);
        assert_eq!(heard(&rx), None, "it waits for the answer");
        let prompt = &live.items()[0];
        assert!(prompt.app.as_ref().is_some_and(|a| a.prompt) && prompt.title == "Ekin" && prompt.until_ms == Some(1_000 + PROMPT_MS));
        let key = prompt.key;
        assert_eq!(desk.answer(key, true, &mut live, 1_500), None, "a click within a second of it appearing counts for nothing");
        assert_eq!(desk.answer(key + 1, true, &mut live, 2_100), None, "a click painted for another item");
        let saved = desk.answer(key, true, &mut live, 2_100).expect("an answer to keep");
        assert_eq!(saved, ApiApp { name: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into(), allowed: true });
        assert_eq!(heard(&rx).map(|h| h.0), Some(200), "the held request goes through");
        assert_eq!(live.items().iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Merhaba"], "the prompt gave way to the message");
    }

    #[test]
    fn a_no_is_kept_and_cools_the_exe_down() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"Merhaba"}"#);
        desk.take(c, &mut live, &[], true, 1_000);
        let key = live.items()[0].key;
        let saved = desk.answer(key, false, &mut live, 2_100).unwrap();
        assert!(!saved.allowed);
        assert_eq!(heard(&rx).map(|h| h.0), Some(403));
        assert!(live.items().is_empty());
        let (c, rx) = call(&Caller { app: "Ekin 2".into(), ..ekin() }, r#"{"app":"Ekin 2","title":"t"}"#);
        desk.take(c, &mut live, &[saved], true, 3_000);
        let (status, body) = heard(&rx).unwrap();
        assert_eq!((status, body.contains("permission_cooldown")), (429, true), "another name from the same exe waits ten minutes");
        assert!(live.items().is_empty(), "no new prompt");
    }

    #[test]
    fn another_app_waits_its_turn() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, first) = call(&ekin(), r#"{"app":"Ekin","title":"ilk"}"#);
        desk.take(c, &mut live, &[], true, 1_000);
        let (c, other) = call(&deniz(), r#"{"app":"Deniz","title":"t"}"#);
        desk.take(c, &mut live, &[], true, 1_100);
        let (status, body) = heard(&other).unwrap();
        assert_eq!((status, body.contains("permission_busy")), (429, true));
        let (c, second) = call(&ekin(), r#"{"app":"Ekin","title":"ikinci"}"#);
        desk.take(c, &mut live, &[], true, 1_200);
        assert_eq!(heard(&first).map(|h| h.0), Some(202), "its newer message is held instead");
        assert_eq!(live.items().len(), 1, "still one prompt");
        let key = live.items()[0].key;
        desk.answer(key, true, &mut live, 2_300).unwrap();
        assert_eq!(heard(&second).map(|h| h.0), Some(200));
        assert_eq!(live.items()[0].title, "ikinci");
    }

    #[test]
    fn an_ignored_prompt_cools_down_when_it_goes() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, _rx) = call(&ekin(), r#"{"app":"Ekin","title":"t"}"#);
        desk.take(c, &mut live, &[], true, 1_000);
        let end = 1_000 + PROMPT_MS;
        live.expire(end, None);
        desk.expire(&mut live, end);
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t"}"#);
        desk.take(c, &mut live, &[], true, end + 1);
        assert_eq!(heard(&rx).map(|h| h.0), Some(429));
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t"}"#);
        desk.take(c, &mut live, &[], true, end + COOLDOWN_MS);
        assert_eq!(heard(&rx), None, "asked again ten minutes on");
    }

    #[test]
    fn a_click_goes_to_the_waiting_request_else_to_the_queue() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let ok = allowed(&[ekin()]);
        let ask = r#"{"app":"Ekin","title":"Deploy?","id":"d1","wait":30,"buttons":[{"id":"no","label":"Hayır"},{"id":"yes","label":"Evet","uri":"https://ornek.test"}]}"#;
        let (c, rx) = call(&ekin(), ask);
        desk.take(c, &mut live, &ok, true, 1_000);
        assert_eq!(heard(&rx), Some((0, "shown".into())));
        let key = live.items()[0].key;
        assert_eq!(desk.click(key, 1, &mut live, 1_500), None, "too soon");
        assert_eq!(desk.click(key, 1, &mut live, 2_100).as_deref(), Some("https://ornek.test"));
        let (status, body) = heard(&rx).unwrap();
        assert!(status == 200 && body.contains(r#""button":"yes""#) && body.contains(r#""id":"d1""#), "{body}");
        assert!(live.items().is_empty(), "a notification goes once answered");
        // Nobody waits: the click is kept for /v1/events.
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t","id":"d2","buttons":[{"id":"ok","label":"Tamam"}]}"#);
        desk.take(c, &mut live, &ok, true, 3_000);
        drop(rx);
        let key = live.items()[0].key;
        assert_eq!(desk.click(key, 0, &mut live, 4_100), None, "no URI");
        let (c, events) = events_call(&ekin(), 0);
        desk.take(c, &mut live, &ok, true, 4_200);
        let (status, body) = heard(&events).unwrap();
        assert!(status == 200 && body.contains(r#""id":"d2""#) && body.contains(r#""button":"ok""#), "{body}");
        let (c, again) = events_call(&deniz(), 0);
        desk.take(c, &mut live, &allowed(&[deniz()]), true, 4_300);
        assert!(heard(&again).unwrap().1.contains(r#""events":[]"#), "another app's clicks aren't its own");
    }

    #[test]
    fn a_wait_for_events_gets_the_next_click() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let ok = allowed(&[ekin()]);
        let (tx, _rx) = channel();
        let mut req = crate::activities::api::parse_activity("yedek", br#"{"app":"Ekin","title":"Yedekleme","buttons":[{"id":"stop","label":"Durdur"}]}"#).unwrap();
        req.id = Some("yedek".into());
        desk.take(ApiCall { caller: ekin(), what: ApiWhat::Show { req: Box::new(req), icon: None, exe_icon: None }, reply: tx }, &mut live, &ok, true, 1_000);
        let (c, events) = events_call(&ekin(), 30);
        desk.take(c, &mut live, &ok, true, 1_100);
        assert_eq!(heard(&events), Some((0, "shown".into())));
        let key = live.items()[0].key;
        desk.click(key, 0, &mut live, 2_100);
        let (status, body) = heard(&events).unwrap();
        assert!(status == 200 && body.contains(r#""button":"stop""#), "{body}");
        assert_eq!(live.items().len(), 1, "an activity stays: the app decides");
    }

    #[test]
    fn a_waited_request_hears_when_its_item_goes() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let ok = allowed(&[ekin()]);
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t","id":"a","wait":60,"duration":5}"#);
        desk.take(c, &mut live, &ok, true, 1_000);
        heard(&rx);
        live.expire(6_000, None);
        desk.expire(&mut live, 6_000);
        let (status, body) = heard(&rx).unwrap();
        assert!(status == 200 && body.contains(r#""event":null"#), "{body}");
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t","id":"b","wait":60}"#);
        desk.take(c, &mut live, &ok, true, 7_000);
        heard(&rx);
        let key = live.items()[0].key;
        desk.dismissed(key, &mut live, 8_100);
        let (_, body) = heard(&rx).unwrap();
        assert!(body.contains(r#""type":"dismissed""#), "{body}");
    }

    #[test]
    fn waiters_whose_time_is_up_are_let_go() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let ok = allowed(&[ekin()]);
        // A progress updater waiting a second on each update: its waiters don't pile up.
        for n in 0..50u64 {
            let (tx, _rx) = channel();
            let mut req = crate::activities::api::parse_activity("yedek", br#"{"app":"Ekin","title":"t","wait":1,"buttons":[{"id":"stop","label":"Durdur"}]}"#).unwrap();
            req.id = Some("yedek".into());
            desk.take(ApiCall { caller: ekin(), what: ApiWhat::Show { req: Box::new(req), icon: None, exe_icon: None }, reply: tx }, &mut live, &ok, true, 1_000 + n * 100);
        }
        assert_eq!(desk.waiting(), 50);
        desk.expire(&mut live, 3_000);
        assert!(desk.waiting() < 50, "the first ones' time is up");
        desk.expire(&mut live, 1_000 + 49 * 100 + 2_001);
        assert_eq!(desk.waiting(), 0);
        assert_eq!(live.items().len(), 1, "the activity itself stays");
    }

    #[test]
    fn mute_takes_the_apps_items_away() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let ok = allowed(&[ekin(), deniz()]);
        for (who, id) in [(ekin(), "a"), (ekin(), "b"), (deniz(), "c")] {
            let (c, _rx) = call(&who, &format!(r#"{{"app":"{}","title":"t","id":"{id}"}}"#, who.app));
            desk.take(c, &mut live, &ok, true, 1_000);
        }
        let key = live.items()[0].key;
        assert_eq!(desk.mute(key, &mut live), Some(ApiApp { name: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into(), allowed: false }));
        assert_eq!(live.items().iter().map(|i| i.app.as_ref().unwrap().name.as_str()).collect::<Vec<_>>(), ["Deniz"]);
    }

    #[test]
    fn denied_off_and_clear() {
        let (mut desk, mut live) = (Desk::default(), LiveSet::default());
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t"}"#);
        desk.take(c, &mut live, &[ApiApp { allowed: false, ..allowed(&[ekin()]).remove(0) }], true, 1_000);
        assert_eq!(heard(&rx).map(|h| h.0), Some(403));
        let (c, rx) = call(&ekin(), r#"{"app":"Ekin","title":"t"}"#);
        desk.take(c, &mut live, &allowed(&[ekin()]), false, 1_000);
        assert_eq!(heard(&rx).map(|h| h.0), Some(503), "Pebbly is switched off");
        let (tx, rx) = channel();
        let req = crate::activities::api::parse_activity("yedek", br#"{"app":"Ekin","state":"clear"}"#).unwrap();
        desk.take(ApiCall { caller: ekin(), what: ApiWhat::Show { req: Box::new(req), icon: None, exe_icon: None }, reply: tx }, &mut live, &[], true, 1_000);
        assert_eq!(heard(&rx).map(|h| h.0), Some(200), "an unknown app clearing nothing isn't asked");
        assert!(live.items().is_empty());
    }
}
