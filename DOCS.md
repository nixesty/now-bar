# Pebbly app notifications API

Your app can show things in Pebbly, the pill in the Windows taskbar: a **notification** that
comes and goes, or a **live activity** you keep updating (a backup at 40 %, a build, a download),
with your icon, your colour and up to two buttons. It takes one HTTP request on this computer.

```bash
curl -H "Content-Type: application/json" -d "{\"app\":\"Backup\",\"title\":\"Backup done\",\"text\":\"12 files, 48 MB\"}" http://127.0.0.1:20034/v1/notify
```

- **Local only.** Pebbly listens on `127.0.0.1` and `[::1]`, never on the network.
- **Off until the user turns it on**, in the setup ("Uygulama bildirimlerini aç" / "Turn on app
  notifications") or in Settings → System → App notifications. Off, nothing listens.
- **The user says yes to each app.** The first request from your app shows a prompt in the pill;
  nothing of yours shows until the user allows it.
- Needs Pebbly, or Now Bar 0.3.0 or later (Pebbly's name before).

## Responsibility

Pebbly only shows what apps send. **Which apps the user allows, and what those apps show,
is the user's responsibility**; what an app sends is its developer's. The setup and the settings
say so where the user turns the API on. Don't use it for anything a user could mistake for
Windows or for another app: the card always says who sent it (below).

## Finding Pebbly

1. **The address is fixed:** `http://127.0.0.1:20034`. Use `127.0.0.1` rather than `localhost`.
2. **If that port is taken** (another program, a Hyper-V reservation), Pebbly listens
   wherever Windows puts it and says where in `%LOCALAPPDATA%\Pebbly\api.json`, which exists
   only while it listens (Now Bar wrote `%LOCALAPPDATA%\NowBar\api.json`):

   ```json
   {"name":"Pebbly","api":1,"version":"0.3.0","url":"http://127.0.0.1:20034","port":20034,"pid":4242}
   ```

   A robust client reads `url` from that file and falls back to `http://127.0.0.1:20034`.
3. **To check it is Pebbly:** `GET /v1` answers `{"name":"Pebbly","api":1,"version":"0.3.0"}`
   (`"Now Bar"` before the rename; the API is the same).

**Connection refused** means Pebbly isn't installed, isn't running, or the user hasn't turned
the API on. Skip quietly; don't ask the user to install anything.

## Permission

Pebbly knows your app by **the name you send (`app`) and the exe your request comes from**
(it looks up the process behind the connection). Both count: the same name from another exe is
another app.

The first request from an app the user hasn't answered opens a prompt in the pill: your exe's
icon, your name, "Wants to show notifications", where your exe is, and Deny / Allow. Your request
waits for the answer up to 20 seconds:

| The user | Your request gets | Then |
|---|---|---|
| allows | `200`, and your message shows | every request goes through |
| denies | `403 denied` | every request gets `403 denied`, without asking, until the user changes it in the settings |
| doesn't answer in 20 s | `202 {"ok":false,"pending":true}` | the prompt stays 2 minutes; if allowed then, your latest message shows |

- One prompt at a time: while another app's prompt is up you get `429 permission_busy`
  (`Retry-After: 10`).
- After a prompt from your exe is denied or ignored, your exe gets no new prompt for 10 minutes,
  under any name: `429 permission_cooldown` (`Retry-After: 600`).
- The user can mute your app from its card ("Mute") and allow, block or forget it in
  Settings → System. A forgotten app is asked again.

## Requests

Every request:

- goes to the address above, with a `Host` of `127.0.0.1`, `localhost` or `[::1]` and the port
  (any HTTP client does this);
- with a body, sends it as JSON: `Content-Type: application/json`, UTF-8, with a `Content-Length`
  or `Transfer-Encoding: chunked`, at most 96 KB, all of it within 5 seconds (curl on Windows
  sends `-d "…"` in the system's code page, not UTF-8: with letters beyond English, put the JSON in
  a UTF-8 file and send `--data-binary @body.json`);
- is one request per connection (`Connection: close`).

Unknown fields are ignored. Text that is too long is cut with "…".

### `POST /v1/notify`: a notification

It leads the pill while it is on (and comes down from the top of the screen over fullscreen apps),
then leaves by itself. A click on one of its buttons answers it and it goes.

| Field | | |
|---|---|---|
| `app` | required | Your app's name as the user sees it, 1–32 characters. Names that would pass for Windows, Pebbly or Claude are refused (`400`, `detail: "app"`): once only letters and digits are kept, they may not contain `pebbly`, `nowbar`, `windows`, `microsoft`, `defender`, `claude`, `anthropic` or `samsung`, in any case. Invisible or direction characters, spaces other than a plain one, `·` and `•` are refused too. |
| `title` | required | Up to 40 characters. |
| `text` | | Up to 120 characters: a line in the pill, up to three on the card. |
| `id` | | Your id for it, 1–40 of `A–Z a–z 0–9 . _ -`. A notification with the same id replaces it. Without one, Pebbly gives it one (in the answer). |
| `duration` | | Seconds on screen, 2–600. Default 5, or 60 with buttons. |
| `icon` | | See [Icons](#icons). Default: your exe's own icon. |
| `accent` | | `"#RRGGBB"`: the circle, the last button, the ring. Default: the user's accent. |
| `buttons` | | See [Buttons and waiting](#buttons-and-waiting). |
| `wait` | | 1–120 seconds: the answer waits for a click. |

```json
{"ok":true,"id":"n7"}
```

### `PUT /v1/activities/{id}`: a live activity

Create it and update it with the same `id` (1–40 of `A–Z a–z 0–9 . _ -`). It stays until you clear
it, until it has been done for 5 seconds, or until an hour passes without an update.

| Field | | |
|---|---|---|
| `app`, `title`, `text`, `icon`, `accent`, `buttons`, `wait` | | As above; `title` isn't needed for `clear`. |
| `state` | | `run` (default): it runs; the pill shows the time it has taken, or the progress. `wait`: it waits for the user ("Waiting", in the accent, with a ring; it comes forward for a moment). `done`: "Done" for 5 seconds, then it goes. `clear`: it goes now. |
| `progress` | | 0–100: a percentage in the pill and a bar on the card. |

```bash
curl -X PUT -H "Content-Type: application/json" -d "{\"app\":\"Backup\",\"title\":\"Backing up\",\"text\":\"Documents\",\"progress\":40}" http://127.0.0.1:20034/v1/activities/backup
curl -X PUT -H "Content-Type: application/json" -d "{\"app\":\"Backup\",\"title\":\"Backup done\",\"state\":\"done\"}" http://127.0.0.1:20034/v1/activities/backup
```

Your ids are yours: another app can't update, clear or read your items, and you can't touch
theirs, the `pebbly.exe push` ones or Claude Code's. An app has at most 3 items at a time (a 4th
takes the place of its quietest); Pebbly keeps 8 in all.

### `POST /v1/events`: what the user did

The clicks and dismissals no `wait` took, oldest first. At most 32 are kept per app, for 10
minutes each.

| Field | | |
|---|---|---|
| `app` | required | Your app's name. |
| `wait` | | 0–60 seconds: wait for the next one when there is none (long polling). |

```json
{"ok":true,"events":[{"type":"button","id":"deploy","button":"yes"},{"type":"dismissed","id":"n7"}]}
```

`id` is your id for the item. An app the user hasn't allowed gets `{"ok":true,"events":[]}`; a
blocked one gets `403 denied`.

### `GET /v1`

```json
{"name":"Pebbly","api":1,"version":"0.3.0"}
```

## Icons

| `icon` | |
|---|---|
| (none) | Your exe's own icon, as Explorer shows it. |
| `"glyph:E8B7"` | A symbol from Windows' icon font (Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10), by its code point (E000–F8FF), white in your accent's circle. |
| `"data:image/png;base64,…"` or `"data:image/jpeg;base64,…"` | A picture, at most 64 KB and 512×512 px (its size is read before it is decoded); square ones look best. Drawn with rounded corners. |

The prompt always shows your exe's icon, whatever you send.

## Buttons and waiting

Up to two buttons, on the card (the user opens it from the pill); the last one is in your accent.

```json
"buttons": [{"id": "no", "label": "Not now"}, {"id": "yes", "label": "Deploy", "uri": "https://example.com/deploys/42"}]
```

- `id`: 1–32 of `A–Z a–z 0–9 _ -`, different from each other. `label`: up to 16 characters.
- `uri` (optional): opened when the button is clicked: `http`, `https`, `mailto` or your app's own
  registered protocol (`yourapp://…`). `file:`, `shell:`, `ms-…`, `search-ms:`, `javascript:`,
  `vbscript:`, `data:` and the like are refused (`400`, `detail: "uri"`).
- A click counts once the item has been on screen for a second (a click aimed at what was there
  before lands on nothing).

**To hear the click, wait for it:** send `wait` (seconds) with the request; the answer comes when
the user clicks a button or closes the card with ×, or when the time runs out or the item leaves:

```json
{"ok":true,"id":"deploy","event":{"type":"button","id":"deploy","button":"yes"}}
{"ok":true,"id":"deploy","event":{"type":"dismissed","id":"deploy"}}
{"ok":true,"id":"deploy","event":null}
```

If the app isn't allowed yet, the first 20 seconds of a `wait` are the prompt's (see
[Permission](#permission)); allow for both in your client's timeout (up to 140 s). A click that
no waiting request takes goes to [`/v1/events`](#post-v1events-what-the-user-did).

Don't take a click alone as the go-ahead for something that can't be undone: any program running
as the user could have answered in Pebbly's place (see [Security](#security)).

## Errors

The body is always JSON: `{"ok":false,"error":"…","detail":"…"}` (`detail` only sometimes).

| Status | `error` | |
|---|---|---|
| `202` | | `{"ok":false,"pending":true}`: the user hasn't answered your app's prompt yet. |
| `400` | `bad_request` | `detail` names the field: `json`, `app`, `title`, `id`, `icon`, `accent`, `buttons`, `uri`, `state`, `progress`, `wait`, `duration`, … |
| `403` | `denied` | The user said no to your app. |
| `403` | `forbidden` | A browser, another Windows user's program, or a caller that can't be told. |
| `404` | `not_found` | |
| `405` | `method_not_allowed` | |
| `411` | `length_required` | A body without a length. |
| `413` | `too_large` | Over 96 KB, an icon over 64 KB or 512 px. |
| `415` | `unsupported_media_type` | Not `application/json`. |
| `429` | `rate_limited`, `permission_busy`, `permission_cooldown` | See `Retry-After`. |
| `503` | `disabled` | Pebbly is switched off in its settings (or, with `detail: "busy"`, 16 requests are open). |

## Limits

| | |
|---|---|
| Requests | 10 a second per app, in bursts of up to 20 (`429 rate_limited`) |
| At once | 16 connections |
| Head, body | 8 KB, 96 KB |
| Items | 3 per app, 8 in all |
| Notification | 2–600 s on screen |
| `wait` | 120 s (`/v1/events`: 60 s) |

## Security

What Pebbly keeps out:

- **Web pages.** A page can't reach the API: requests with `Origin`, `Sec-Fetch-Site` or
  `Sec-Fetch-Dest` (which browsers send) are refused, JSON can't be sent from a page without a
  preflight that never gets CORS headers, and a `Host` other than the loopback names is refused.
  So code in a browser, a browser extension or an Electron renderer can't call it: call it from
  your app's own process (Node's `fetch` in Electron's main process works).
- **The network.** It listens on loopback only.
- **Other Windows users** on the same computer: a request from another session is refused.
- **Look-alikes.** The card always shows who sent it: your app's name and your exe's file name
  ("Backup · backup.exe"), which you can't change. Names that would pass for Windows,
  Pebbly or Claude are refused, and so are button URIs that open files or run things.
- **Floods.** The rate limit, 3 items per app, one prompt at a time, and a 10-minute cooldown.

What it can't: a program already running as the user can do what the user can, so the permission
is about consent and noise, not a wall against malware. Such a program could also take port
20034 before Pebbly starts, or answer a `wait` in its place.

Pebbly's log keeps your app's name, your exe's file name and the item's id, never the text.

## Examples

### PowerShell

```powershell
$body = @{ app = "Backup"; title = "Backup done"; text = "12 files, 48 MB" } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:20034/v1/notify" -ContentType "application/json; charset=utf-8" -Body ([Text.Encoding]::UTF8.GetBytes($body))
```

(The bytes keep non-English letters intact on Windows PowerShell 5.1.)

### Python (standard library)

```python
import json, os, urllib.error, urllib.request

def pebbly_url():
    try:
        with open(os.path.expandvars(r"%LOCALAPPDATA%\Pebbly\api.json"), encoding="utf-8") as f:
            return json.load(f)["url"]
    except (OSError, ValueError, KeyError):
        return "http://127.0.0.1:20034"

def pebbly(method, path, body):
    req = urllib.request.Request(pebbly_url() + path, data=json.dumps(body).encode(), method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=150) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        return json.load(e)          # 403, 429, … with the reason
    except OSError:
        return None                  # no Pebbly, or the API is off

pebbly("PUT", "/v1/activities/backup", {"app": "Backup", "title": "Backing up", "progress": 40})
answer = pebbly("POST", "/v1/notify", {
    "app": "Backup", "title": "Delete old backups?", "wait": 60,
    "buttons": [{"id": "keep", "label": "Keep"}, {"id": "delete", "label": "Delete"}],
})
if answer and (answer.get("event") or {}).get("button") == "delete":
    ...
```

### Node (18 or later)

```js
const r = await fetch("http://127.0.0.1:20034/v1/notify", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ app: "Backup", title: "Backup done", text: "12 files, 48 MB", accent: "#1DB954" }),
}).catch(() => null); // no Pebbly
console.log(r && (await r.json()));
```

### C#

```csharp
using System.Net.Http.Json;

var http = new HttpClient { BaseAddress = new Uri("http://127.0.0.1:20034"), Timeout = TimeSpan.FromSeconds(150) };
try
{
    var r = await http.PostAsJsonAsync("/v1/notify", new { app = "Backup", title = "Backup done", text = "12 files, 48 MB" });
    Console.WriteLine(await r.Content.ReadAsStringAsync());
}
catch (HttpRequestException) { } // no Pebbly
```

## From a script: `pebbly.exe push`

Scripts you run yourself can also use Pebbly's own exe, without HTTP and without a prompt
(it needs Settings → System → Live activities, which is on by default):

```bash
pebbly.exe push --id backup --title "Backup" --text "Documents" --progress 40
pebbly.exe push --id backup --title "Backup" --state done
```

`--state` is `run`, `wait`, `done` or `clear`, as above. It has no icons, colours or buttons, and
exits at once (always 0) when Pebbly isn't running.

## Versions

`api` is `1`. New fields and endpoints may come within it; nothing in it changes meaning. Anything
that would is `/v2`.
