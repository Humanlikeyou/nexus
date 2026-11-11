# HANDOFF-1111w: Jarvis windows on Home (for Claude)

**Status:** built on design-build-1111w (see §8). Mockups remain in `shots/` and `compare/`.
- **Base:** amara-integration c888c52 (includes 1111v). The mocks predate Taisun's 19:44 JST picks; §8 and the Built app follow the picks where they differ.
- **Choices taken from the brief:**
  - Close by voice ("close it", or naming the window) or by X.
  - Home slides back when the last window closes.
  - Sign in once inside Amara, using a persistent webview profile per site.
- **Command names:** names written in `code` are proposals; the Lead decides the final ones.

## 1 · Behaviour
- **Trigger:**
  - "pull up / show me / open …" by voice or chat calls **one backend command**. This is the 1111v single write path: chat and voice call the same command.
  - Proposed: `home_window_open { kind: web | video | chart | pdf | social, target: url | query | localPath }`.
  - Sequence: Home shows **Hearing…**, then the parsed result as a chip ("→ Opening TradingView · BTCUSD, 4 hours"), then the window appears.
  - Opening a window is not destructive, so there is no confirm step. The chip is the read-back.
- **Limit and layout:**
  - At most **3** windows on the stage.
  - The newest takes the big slot (left, 1.618) and gets focus; older ones move to the right column.
  - With 3 windows, the right column splits 1.618 : 1.
  - Gaps are 21 px, and the stage padding is 13 / 34 / 21.
  - The rail shows the orb, Amara's state, "Windows N of 3", Close all and the time.
  - The command bar at the bottom is the Home composer in compact form.
- **4th request:**
  - It replaces the **oldest-opened** window, with the toast "Closed X (the oldest) to make room for Y · Undo · 8 s".
  - Undo brings X back and closes Y.
- **Focus:**
  - The newest window gets focus.
  - Clicking a window, saying its name ("make the video big") or pressing ⌥1–3 focuses it or swaps it into the big slot.
  - "Close it" and Esc act on the focused window.
- **Close:**
  - Triggers: X, Esc, ⌘W, "close it", "close the chart / video / PDF / Instagram", or "close everything".
  - ⌘Z reopens the last window within 8 s.
  - When the last window closes, Home returns.
- **Expand / pop out:**
  - Expand fills the stage; the others are listed in the rail as "+2 hidden".
  - Pop out moves the webview into its own native window (`WebviewWindow`). It leaves the stage and does **not** count toward the 3 (question for Taisun).
- **Caption strip:** one per window, in this order:
  1. a working line while she reads ("Reading the chart…")
  2. her short comment, spoken and shown as text, with Replay
  3. a source note ("Local · light tier · from what's on screen")
  - Small windows clamp the text to 2 lines.
  - She makes **no** comment in these states: signed out, blocked, offline, or a sign-in page.
  - Comments describe what's on screen and never give a trading call.
- **PDF:**
  - The summary has 3 bullets, plus any dates it found as an "Add … to Dates & deadlines…" button.
  - That button uses the 1111v add pattern: preview → confirm → Undo.

## 2 · Motion (app motion tokens; see `w18`)
| Move | Standard | Reduced motion |
|---|---|---|
| Home exits | Greeting column 34 px left + fade; chat and buttons 21 px down + fade (180 ms ease-out); lens shrinks into the orb (320 ms spring) | crossfade, 150 ms |
| Window 1 in | Grows from the orb: scale 0.92 → 1, 21 px drop, fade (320 ms spring, from 120 ms) | fade, 150 ms |
| Window 2 / 3 in | Slides 55 px from the right / rises 34 px; the others re-flow (240 ms) | fade; layout changes instantly |
| Replace, close | Scale 0.96 + fade (180 ms), then re-flow (240 ms) | fade |
| Last close | Exit in reverse (320 ms) | crossfade |

Home content that is moving out is `inert` and `aria-hidden`. With Reduce transparency on, the windows use `--glass-solid` with no blur.

## 3 · Webviews and sign-in (src-tauri: Claude / Lead; not part of this UI pass)
- **Webviews, not iframes:**
  - Use child webviews positioned over the stage rects, or `WebviewWindow` for pop-out. This matters for sites that refuse framing (Instagram, TradingView).
  - The UI sends slot rects on layout or resize; the backend moves the webviews.
  - Animate a snapshot or placeholder in the UI, then show the real webview at rest. Native webviews can't be CSS-animated.
- **One persistent profile per site:**
  - The store lives in the app data folder, e.g. `…/com.taisun.nexus/webviews/<site>/`.
  - On macOS this needs a per-site WebKit data store (a data-store identifier). Check what the Tauri version in use supports before building.
  - Session data never goes into chat, logs, memory or Letta.
  - Settings › Services › *Sites you're signed in to inside Amara* (`w20`) lists sites with Signed in / Not signed in and Sign out. Sign out deletes that site's profile.
  - No cookie or token values are ever shown.
- **Sign-in:**
  - It's the site's own page in its profile (`w11`). Amara **pauses reading** whenever the page has a password field or a known login URL.
  - Logs strip query strings and fragments from URLs.
- **Blocked embed:** if a site still refuses the child webview or a policy blocks it, show "This site won't open inside Home" with the button "Open in pop-out window" (`w12`).
- **Other states:**
  - Loading: a skeleton plus "Loading <host>…".
  - Offline: "You're offline", the last loaded time, and Try again.
  - Signed out: "Sign in once inside Amara".

## 4 · Reading the content and privacy
- **Text comes from this Mac only:**
  - Web pages: visible text read from the webview DOM by a read-only injected script.
  - PDFs: the PDF text layer.
  - Charts: on-screen text and the visible price axis.
  - Nothing is uploaded.
- **Model:** comments use the **local light tier**; cloud stays off by default.
- **`NEXUS_OUTBOUND_DISABLED`** is respected for Amara's own calls: she never fetches pages herself. Pages he opens load normally.
- **Read-only:** Amara never clicks, posts, likes or types on sites.

## 5 · Accessibility and keyboard (`w19`)
- Each window is `role="region"` with an accessible name ("TradingView window").
- Controls: Pop out, Expand / Restore and Close, each named with its window ("Close TradingView (Esc)").
- Captions are a polite live region, and the spoken comment is always also text.
- **Focus moves:**
  - On open, focus goes to the new window's title.
  - On close, it returns to the previous window or the Home composer.
- **Keys:**
  - Esc / ⌘W: close
  - ⌥1–3: focus a window
  - Tab: cycle through windows, then their controls
  - ⌘⇧↩: expand
  - ⌘⇧O: pop out
  - ⌘Z: reopen
- These keys must not clash with ⌘1, which focuses the chat on Home.

## 6 · Works with 1111v either way
The stage replaces the landing content during window mode and restores it untouched. The landing nav row follows `SYSTEMS`, so it shows 6 buttons today or 7 with 1111v's Life (`w16b`). Needs you today lives on Today, not on the landing, so nothing on Home depends on 1111v.

## 7 · Tests when building
- **Layout:** a reducer for 1 → 2 → 3 → replace-oldest → undo → close → last-close returns Home.
- **Focus:** the focus rules, plus voice targets ("close the chart" resolves by kind or site).
- **Comment timing:** no comment while signed out, blocked, offline or on a login page.
- **Settings:** the sites list shows status only, with no secret values.
- **Commands:** `commands.registered.test.ts` stays green, with UNWIRED adapters until the backend lands.
- **e2e:** an axe sweep in dark and light, a reduced-motion snapshot, and keyboard close with Esc.

## 8 · Built (build-1111w, branch design-build-1111w)

**Approved:** 19:44 JST, 11 Nov 2026. Base: `amara-integration` c888c52 (includes 1111v). UI only, in `src-console` (plus e2e specs and this file under `docs/design/`). No src-tauri changes. The commit hash, install time (JST) and pid will be in `/workspace/nexus/build-1111w/README.md`.

**Taisun's picks (these override the mockups and earlier sections of this handoff):**
- Popped-out windows **count** toward the max of 3.
- A 4th window replaces the window he **looked at least recently**, not the oldest opened. The 8 s Undo toast stays; the copy is "Closed X (looked at least recently) to make room for Y".
- Amara **speaks** her comment only when he asked by voice. When he typed in chat, the caption is shown and Replay is hidden.
- Windows stay **dark** in light theme (Home is already always dark). The mock light-glass treatment is not used.
- Close by voice or the X (also Esc / ⌘W).
- Sign in once inside Amara, on the site's own page. Amara reads nothing during sign-in. Settings › Services lists signed-in sites with Sign out, and never shows secrets.

**What was built:**
- Stage on Home (`src-console/homeWindows/`): Hearing → understood chip → Home exits → up to 3 windows in a 1.618 layout, rail (orb, state, "Windows N of 3", Close all, time), compact command bar, captions, 8 s Undo toast, expand, pop-out (still counted), last close returns Home.
- Sample bodies (chart, video, social, PDF, web) and states (signed out, sign-in page, blocked, offline, Needs wiring) run only in sample mode (`nexus.sampleAdapters=1`). Mid-frame previews use `nexus.homeWindowsPreview`.
- Chat and the compact bar share `handleWindowPrompt`; new spoken turns after mount are treated as voice (historical polls are skipped). "pull up / show me / open / bring up" opens; "close it / close the chart / close everything / make X big / expand / undo" act on the stage.
- Settings › Services › *Sites you're signed in to inside Amara*: host, status, Sign out. No cookies, tokens or passwords.
- Motion tokens as in §2; reduced motion is a 150 ms crossfade only. Home content is `inert` while windows are up.
- PDF caption: 3 bullets plus "Add … to Dates & deadlines…" using the 1111v add preview pattern.


**Added after first install (Taisun, same evening):** Life shows Amara's take. `TakePage` now includes `"life"`. `<AmaraTake page="life" />` sits under the hero on Goals (the Life landing), Weekly review, Dates & deadlines, and Personal money. Same component and Wired adapter as Fitness, Neuro, Mind–Body and Enterprise. Live facts return `null` (Needs wiring, no invented numbers). Sample mode uses a Life note tagged Sample data. Personal money still also has the 1111t `MoneyTake` (page="business") under the Life take.

**Deviations:**
1. Toast copy uses "looked at least recently" (the w09 shot filename is still `fourth-replaces-oldest`).
2. True native webview embedding and pop-out `WebviewWindow` are not in this UI pass (would need src-tauri). The pop-out control marks the window popped on the stage; live embedding is a Needs wiring adapter.
3. Live TTS, on-screen comments, and the session store are UNWIRED. Sample comments are tagged Sample data and are never a trading call.
4. The compact command-bar mic is disabled until voice capture is wired. Typed commands still work.
5. w12 / w18 / w19 are mock boards, not live routes, so they are not captured from the app.
6. The sign-in "page" in sample mode is a labeled placeholder of the site's own form; fields are disabled and collect nothing. A real sign-in page needs the webview.

**Backend needs (for Claude / the Nexus Lead) — no src-tauri was changed here:**
1. **Webview embedding:** child webviews positioned over stage rects (the UI can send slot rects on layout/resize). Native webviews can't be CSS-animated; snapshot/placeholder in the UI, real webview at rest. Sites that refuse framing (Instagram, TradingView) need this, not iframes.
2. **Pop-out:** `WebviewWindow` for a popped-out window; it still counts toward the cap of 3.
3. **Per-site persistent profile** in the app data folder (e.g. `…/com.taisun.nexus/webviews/<site>/`), with a macOS per-site WebKit data store. Session data must never go into chat, logs, memory or Letta. Sign out deletes that site's profile.
4. **Sign-in pause:** Amara must not read the page when it has a password field or a known login URL. Logs strip query strings and fragments.
5. **`home_window_open` (proposed)** `{ kind, target }` as the one write path for voice and chat.
6. **On-device comments** (local light tier, from visible text on this Mac only) and **TTS**. Speak only when `openedBy === "voice"`.
7. **Blocked / offline** signals from the webview so those states are live, not only sample previews.
