# Nexus — Project Brief for Claude Coders

> **Central Goal**: Nexus is a personal AI-powered command center — a Tauri desktop app that aggregates your entire life into one dashboard. It unifies AI agent management (Terrance + OpenClaw), neuro/biometric monitoring (Muse EEG, Whoop), business intelligence (ARM trading system), fitness tracking, and an AI assistant — all orchestrated through n8n automation workflows and surfaced in a tactical command interface (dark graphite, precision-engineered panels, desaturated emerald accents).

---

## Architecture Overview

```
nexus/
├── src/                        # React + TypeScript frontend
│   ├── App.tsx                 # Root — page routing via sidebar, 8 pages
│   ├── components/
│   │   ├── layout/             # Sidebar (icon rail), Shell, Header
│   │   ├── system/             # OpenClawStatusCard
│   │   ├── terrance/           # AI agent monitoring (StatusCard, TokenUsage, etc.)
│   │   ├── arm/                # Trading system (RiskPanel, PositionsTable, etc.)
│   │   ├── neuro/              # EEG/brain monitoring (20+ components)
│   │   ├── fitness/            # Whoop integration (StrainChart, RecoveryScore, etc.)
│   │   ├── business/           # Business metrics
│   │   ├── trajectory/         # Goal tracking
│   │   ├── assistant/          # AI chat interface
│   │   ├── settings/           # App configuration
│   │   └── ui/                 # Shared: OnboardingModal, CommandPalette, etc.
│   ├── lib/
│   │   ├── types.ts            # All TypeScript types (OpenClawStatus, ArmRiskStatus, etc.)
│   │   ├── chart-theme.ts      # Shared ECharts constants (fontSize, borderRadius, etc.)
│   │   ├── api.ts              # Tauri invoke wrappers
│   │   └── utils.ts            # formatRelativeTime, cn(), etc.
│   ├── terrance/system/        # openclawHealth.ts — pushHealthToN8n()
│   └── styles/globals.css      # Design system: OKLch palette, glass classes, animations
├── src-tauri/
│   ├── src/commands/
│   │   ├── openclaw.rs         # get_openclaw_health(), kill_openclaw(), restart_openclaw()
│   │   ├── whoop.rs            # Whoop BLE streaming commands
│   │   ├── muse.rs             # Muse EEG BLE streaming commands
│   │   ├── arm.rs              # ARM trading system commands
│   │   ├── sessions.rs         # Neuro session recording
│   │   ├── telegram.rs         # Telegram notifications
│   │   └── ...                 # goals, snapshots, replay, packager, outbox, etc.
│   └── Cargo.toml              # Rust dependencies (tauri 2.0, dirs, serde, etc.)
└── CLAUDE.md                   # This file
```

### Tech Stack
| Layer | Technology |
|-------|-----------|
| Desktop shell | Tauri 2.0 (Rust backend + WebView frontend) |
| Frontend | React 18, TypeScript, Vite, Tailwind CSS 4 |
| UI components | Radix UI, Framer Motion, ECharts (echarts-for-react) |
| Rust backend | Tauri commands, `dirs`, `serde_json`, `tokio` |
| Automation | n8n (self-hosted, Docker stack) |
| AI gateway | OpenClaw (Node.js, port 18789, LaunchAgent) |
| Biometrics | Muse EEG via BLE, Whoop via BLE |
| Trading | ARM system via local HTTP |
| Notifications | Telegram Bot API |

---

## Development Commands

```bash
# Start the app in dev mode (hot-reload frontend, watch Rust)
cd /Users/thevisualdude/projects/nexus
npm run tauri -- dev

# TypeScript type check (no emit)
npx tsc --noEmit

# Frontend only (Vite dev server, no Tauri window)
npm run dev

# Build production app
cargo tauri build
```

**Dev server**: Frontend at `http://localhost:1420` with Vite HMR.
**Rust changes**: Require full app restart (Cargo recompiles automatically with `tauri dev`).
**Frontend changes**: Picked up instantly via Vite HMR — no restart needed.

---

## Services & Infrastructure

| Service | Location | Notes |
|---------|----------|-------|
| OpenClaw AI gateway | `127.0.0.1:18789` | LaunchAgent `ai.openclaw.gateway`, module at `/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs` |
| n8n automation | Docker: `stack-n8n-1` | Backed by `stack-postgres-1`, db `n8n_db`, user `terrance` |
| Postgres | Docker: `stack-postgres-1` | `psql -U terrance -d n8n_db` |
| Terrance integrations | `/Users/thevisualdude/terrance-integrations/` | n8n workflow JSON files in `stack/integrations/n8n/` |

### OpenClaw Health Check Logic (Rust)
`src-tauri/src/commands/openclaw.rs` — `check_memory_search()`:
1. Reads `~/.openclaw/config.json` — looks for `modelPath` or `localModel` keys
2. **If absent**, probes `~/.node-llama-cpp/models/` for any `.gguf` file as fallback
3. Returns `"operational"` if found, `"missing"` if not

This was needed because `openclaw memory status --deep` downloads the model but doesn't write `modelPath` to the config file.

---

## n8n Workflows

### openclaw_health_monitor.json
- **Location**: `/Users/thevisualdude/terrance-integrations/stack/integrations/n8n/openclaw_health_monitor.json`
- **Workflow ID**: `tAKyEzsax7BoTM7t`
- **Trigger**: Webhook POST from Nexus app (via `pushHealthToN8n()` in `openclawHealth.ts`)
- **Flow**: webhook → extract status → switch (healthy/degraded/offline) → 3 conditional branches → Telegram alert → respond
- **Telegram credentials**: Bot token and Chat ID are hardcoded in the Code node (real values set Feb 2026)
- **Status**: Active in n8n

### terrance_ingress_workflow.json
- General Terrance AI agent ingress workflow

---

## UI Design System

### Philosophy
Tactical command interface — precision-engineered, restrained, authority. Think luxury aerospace instrumentation. No glass morphism, no pastels, no SaaS card aesthetic. Dark only.

### Key CSS Classes (`src/styles/globals.css`)
```css
.panel          /* Main panel: flat #0F1318 bg, 1px border, inset highlight */
.panel-subtle   /* Lighter panels: semi-transparent, thinner border */
.panel-elevated /* Modals/overlays: slightly lighter, drop shadow */
.panel-primary  /* Dominant system panel: emerald-tinted border + glow */
.glass/.glass-subtle/.glass-elevated  /* Aliases → panel equivalents (backward compat) */
.heading-section  /* ALL CAPS, 11px, tracked, muted */
.metric-value     /* Mono, 24px, semibold — dominant numbers */
.metric-label     /* Mono, 10px, uppercase, desaturated */
.badge-status     /* Mono, 10px, uppercase, tracked, inline-flex */
.btn-kill         /* Deliberate destructive button — mono, bordered */
.subsystem-inactive  /* Dashed border, dimmed — for unconnected modules */
.terminal-readout    /* Dark bg, mono font — for log output */
```

### Color Palette (hex — dark only, contrast-calibrated)
| Token | Value | Use |
|-------|-------|-----|
| `--background` | `#0B0F14` | Near-black base (Layer 1) |
| `--card` | `#131921` | Panel surface (Layer 2) |
| `--foreground` | `#E0E3E9` | Primary text |
| `--muted-foreground` | `#737D8C` | Secondary labels |
| `--accent` | `#4A9E7E` | Calibrated emerald (+15% sat) |
| `--accent-secondary` | `#378A68` | Deeper emerald |
| `--success` | `#4A9E6A` | Lifted green |
| `--warning` | `#A3923D` | Lifted amber |
| `--destructive` | `#A34E4E` | Lifted red |
| `--data` | `#4A80A3` | Cool blue (neuro) |
| `--border` | `#263040` | Visible steel borders |

### Typography Scale
| Old | New | Context |
|-----|-----|---------|
| `text-[9px]` | `text-[11px]` | Micro labels |
| `text-[10px]` | `text-xs` (12px) | Small labels |
| `text-[11px]` | `text-[13px]` | Secondary text |
| `text-sm font-medium` (h3) | `text-base font-medium` | Card headings |

### ECharts Shared Theme (`src/lib/chart-theme.ts`)
```ts
export const CHART_THEME = {
  gauge: { progressWidth: 6, axisLineWidth: 6, titleFontSize: 10, detailFontSize: 24 },
  colors: { phthalo: "#4A9E7E", amber: "#A3923D", cyan: "#4A80A3", solar: "#A3803D" },
  axisLabel: { color: "rgba(139, 149, 163, 0.8)" },
  splitLine: { color: "rgba(38, 48, 64, 0.4)" },
}
```
Thin instrumentation arcs, contrast-calibrated colors aligned to globals.css. All chart components import this.

---

## Sidebar — Icon Rail

`src/components/layout/Sidebar.tsx`
- **Collapsed** (default): 64px wide, icons only, Radix Tooltip on hover
- **Expanded** (mouse enter): 220px wide, icon + text label, framer-motion `animate={{ width }}`
- **Animation**: 250ms cubic-bezier `[0.25, 0.1, 0.25, 1]`
- **Active indicator**: 3px accent left bar + `bg-accent/12` + glow
- **BLE status dots**: Compact stacked circles when collapsed, dot + label when expanded
- **Pages**: Terrance, Business, Revenue, Assistant, Neuro, Fitness, Trajectory, Settings

---

## Completed Work (Session Log)

### OpenClaw Integration
- [x] Updated OpenClaw from v2026.2.6-3 → v2026.2.22
- [x] Built Tauri command `get_openclaw_health()` — checks process, HTTP, config, memory search
- [x] Fixed `check_memory_search()` to probe `~/.node-llama-cpp/models/*.gguf` (config key absent bug)
- [x] Built `OpenClawStatusCard.tsx` — real-time health display with restart/kill controls
- [x] Built `openclawHealth.ts` — `pushHealthToN8n()` sends health payload to n8n webhook
- [x] Wired `OpenClawStatusCard` into Terrance page in App.tsx (2-col grid with GatewayStatus)
- [x] Fixed duplicate import bug in App.tsx (TS2300 → 0 errors)

### n8n Health Monitor Workflow
- [x] Created `openclaw_health_monitor.json` with 7 nodes
- [x] Set real Telegram Bot Token + Chat ID in Code node
- [x] Re-imported workflow, resolved FK constraint on activation, workflow is ACTIVE
- [x] n8n workflow sends Telegram alerts on healthy/degraded/offline status changes

### UI Modernization (Apple Vision Pro aesthetic)
- [x] **globals.css** — Richer OKLch palette, enhanced glass classes, ambient body gradient mesh, shimmer/glow-pulse/gradient-shift animations
- [x] **Sidebar** — Complete icon rail rewrite (64px→220px framer-motion, Radix Tooltips, glow on active)
- [x] **Shell** — Ambient glow orbs (cyan top-left, violet bottom-right), `p-6→p-7`, z-index layering
- [x] **Header** — `glass-subtle`, larger typography (`text-xl`), `px-7 py-5`, `rounded-xl` buttons
- [x] **Typography** — Bulk bump: `text-[10px]→text-xs`, `text-[11px]→text-[13px]`, h3 `text-sm→text-base`
- [x] **Spacing** — `glass p-5→p-6`, `glass p-4→p-5`, `space-y-4→space-y-5`
- [x] **Badges** — `px-1.5 py-0.5 rounded` → `px-2 py-0.5 rounded-md font-medium`
- [x] **Progress bars** — `h-1.5→h-2` across GoalTracker, SessionModePicker, RiskPanel, etc.
- [x] **ECharts** — All 9 charts use `CHART_THEME` constants (axis 11px, tooltip 13px/r12, gauge 32px detail)
- [x] **Dialogs/Modals** — All Radix overlays use `glass-elevated`
- [x] **Settings inputs** — `py-2 rounded-lg` → `py-2.5 rounded-xl`
- [x] **App.tsx grids** — `gap-4→gap-5`, `space-y-6→space-y-7`

### LLM Router (Session Feb 2026)
- [x] `src/lib/types.ts` — Added `ComputeMode`, `LlmRouterConfig`, `RouterResponse`, `LlmUsageRecord`, `CostEstimate`; 8 new `NexusConfig` fields
- [x] `src-tauri/src/commands/llm_router.rs` — Full routing module: `decide_provider()`, `call_ollama()`, `call_anthropic()`, 4 Tauri commands, 16 unit tests
- [x] `src-tauri/src/commands/mod.rs` — Added `pub mod llm_router`
- [x] `src-tauri/src/lib.rs` — Registered 4 commands: `chat_with_router`, `get_cost_estimate`, `get_llm_usage`, `clear_llm_usage`
- [x] `src-tauri/src/commands/telegram.rs` — Added 8 `#[serde(default)]` LLM router fields to `NexusConfig` struct
- [x] `src/lib/llm-router.ts` — TypeScript helpers: `estimateTokens`, `estimateCostUsd`, `formatCost`, `formatProvider`, `buildRouterConfig`, `aggregateUsage`
- [x] `src/lib/api.ts` — Added `chatWithRouter`, `getCostEstimate`, `getLlmUsage`, `clearLlmUsage` wrappers
- [x] `src/components/settings/SettingsPage.tsx` — Compute Mode section: mode selector, model inputs, API key, budget caps, cost toggle, usage summary
- [x] `src/components/terrance/TelegramChat.tsx` — Wired to `chatWithRouter`; shows provider badge + cost estimate per message

### Terrance Continuity Architecture (Session Feb 2026)
- [x] `config/terrance.identity.md` — Canonical identity: operating principles, communication style, decision priorities, tone constraints, mission alignment. Prepended to ALL model calls regardless of provider.
- [x] `src/terrance/system/continuity.ts` — `buildSystemPrompt()` (identity + context primer), `summarizeAndStoreSession()` (post-session memory), `loadContext()` / `saveContext()`, `INFERENCE_PARAMS` (shared temperature/top_p/repeat_penalty)
- [x] `src-tauri/src/commands/continuity.rs` — `read_identity_file`, `load_terrance_context`, `save_terrance_context`, `store_session_log`, `list_session_logs`
- [x] `memory/session_logs/` — Directory for per-session JSON summaries (committed to repo for portability)
- [x] `~/Library/.../terrance_context.json` — Runtime context store: active goals + last session summary
- [x] `src/components/terrance/TelegramChat.tsx` — Identity injected at send time; "Save to memory" button triggers `summarizeAndStoreSession()`; provider badge + cost shown per message

**Continuity guarantees:**
- Identity file is always prepended regardless of mode (local/hybrid/api)
- Context (goals + last summary) loaded from external storage before every session
- Session summaries stored to `memory/session_logs/` after each conversation
- Fallback identity inline if file read fails — Terrance never loses its identity
- Routing never alters the identity prompt — only the compute engine changes

#### LLM Router Test Checklist
- [ ] **Local mode** — Set mode=local, send a message, verify provider badge shows "Local", no API cost logged
- [ ] **Hybrid default** — Set mode=hybrid, send a short message, verify it routes to Ollama
- [ ] **Hybrid tag escalation** — In hybrid mode, if tags include "heavy", verify Anthropic is used (requires API key)
- [ ] **API mode** — Set mode=api with valid API key, send a message, verify Anthropic is used and cost is logged
- [ ] **Budget enforcement** — Set daily cap to $0.00, verify hybrid mode falls back to Ollama (cap exceeded)
- [ ] **Usage log** — After several calls, open Settings → Compute Mode, verify usage summary shows correct counts and costs
- [ ] **Clear usage** — Click "Clear" in usage summary, verify log resets to zero
- [ ] **Offline fallback** — In hybrid mode with API key, kill internet, verify graceful fallback to Ollama (no crash)
- [ ] **Unit tests** — Run `cargo test -p nexus-lib` and verify all 16 router tests pass

---

### OpenClaw Model Routing (Session Feb 25 2026)
- [x] OpenClaw primary switched to `google/gemini-2.0-flash` (temporary — until Anthropic credits topped up)
- [x] Fallback chain: `ollama/qwen2.5:7b → ollama/qwen2.5:32b → anthropic/claude-haiku → anthropic/claude-sonnet`
- [x] `OLLAMA_API_KEY=ollama-local` added to LaunchAgent plist
- [x] `models.json` fixed — was storing literal string "OLLAMA_API_KEY" instead of value
- [x] Sessions cache cleared (was locked on deleted deepseek-r1:32b)
- [ ] **When Anthropic credits topped up**: change primary back to `anthropic/claude-haiku-4-5-20251001` in `~/.openclaw/openclaw.json`

### Visual Redesign: Tactical Command Interface (Session Mar 2026)
- [x] **globals.css** — Complete rewrite: removed glass morphism, OKLch colors → hex, near-black base (#0B0F14), `.panel`/`.panel-primary`/`.panel-elevated` replace `.glass` classes, new utility classes (`.heading-section`, `.metric-value`, `.metric-label`, `.badge-status`, `.btn-kill`, `.subsystem-inactive`, `.terminal-readout`), reduced border-radius (max 6px), removed light mode, restrained animations
- [x] **chart-theme.ts** — Thinner gauges (progressWidth: 6), desaturated colors (#3D8B6E emerald, #8B7A3D amber), darker tooltip/axis styling
- [x] **Shell.tsx** — Removed vignette overlay div, reduced padding (p-6→px-4 py-3)
- [x] **Header.tsx** — Removed glass-subtle, smaller typography (text-xs), mono uppercase tracked headings, tighter layout
- [x] **Sidebar.tsx** — Removed glass class, dark bg (#0A0E13), narrower (56px collapsed/200px expanded), mono uppercase labels, reduced border-radius
- [x] **Terrance page** — Full redesign: SYSTEM CORE dominant full-width panel at top, 3-col secondary modules (Token/Gateway/Skills), 2-col tertiary (OpenClaw/Schedule), full-width chat. All components use `.panel`/`.heading-section`/`.metric-value`/`.metric-label`
- [x] **Business page** — TaskBoard, ContentPipeline, RevenueCard: glass→panel, reduced padding, mono fonts, tighter spacing
- [x] **Assistant page** — CalendarView/EmailSummary use `.subsystem-inactive` (dimmed, dashed border), DailyLog uses `.terminal-readout`
- [x] **Fitness page** — RecoveryScore/SleepChart/StrainChart/HrvTrend: glass→panel, thinner gauge arcs via CHART_THEME
- [x] **Trajectory page** — WellnessScore/GoalTracker: glass→panel
- [x] **ARM/Revenue page** — All 7 arm components: glass→panel (bulk sed)
- [x] **Neuro page** — All 24 neuro components: glass→panel (bulk sed)
- [x] **Settings page** — All sections: glass→panel (bulk sed)
- [x] **TypeScript** — Zero errors (`npx tsc --noEmit` clean)

**Design language**: Near-black graphite base, 1px steel borders, desaturated emerald accent, ALL CAPS tracked headings, JetBrains Mono for metrics, thin instrumentation gauges, no drop shadows, no blur, no pastels, no rounded-xl. Panels feel mounted, not floating.

### Contrast Calibration Pass (Session Mar 2026)
- [x] **globals.css** — Lifted all CSS custom properties: card #0F1318→#131921, foreground #C8CCD4→#E0E3E9, muted-foreground #5C6370→#737D8C, accent #3D8B6E→#4A9E7E (+15% saturation), border #1E2530→#263040 (higher opacity), panel-primary glow strengthened, heading color #5C6370→#8B95A3, metric-value #F0F2F5 (brightest tier), metric-label #454D58→#6B7585, all semantic colors lifted ~15%
- [x] **chart-theme.ts** — Axis labels rgba(92,99,112)→rgba(139,149,163), splitLine rgba(30,37,48)→rgba(38,48,64), tooltip text lifted to match new foreground, chart colors aligned to new accent (#4A9E7E, #A3923D, #4A80A3)
- [x] **Terrance page** — All 6 components manually calibrated: StatusCard (model text /90→100%, heartbeat /70→/85), TokenUsage (chart bar colors updated to new accent), SkillsList (skill names /70→/85, badges /70→100%), GatewayStatus (metric values /50→/75, destructive /60→/80), CronSchedule (job names /70→/85, borders /20→/35), TelegramChat (comprehensive overhaul: timestamps /20→/40, meta /15→/35, bot icons /60→/80, message borders /10→/18, input bg #080B0F→#0C1018, placeholder /20→/35, send button /70→/85, typing dots /30→/45)
- [x] **Header.tsx** — Page title /80→/90, description /50→/65, buttons /50→/65, hover states /70→/80, header border /40→/55
- [x] **Sidebar.tsx** — Sidebar bg #0A0E13→#0C1018, borders /20→/35 and /40→/55, title /70→/85, subtitle /40→/55, version /25→/40, BLE dots /20→/35, tooltip arrow fill #121820→#172030
- [x] **System-wide bulk uplift** — All remaining components (~50 files): text-muted-foreground (/15→/30, /20→/35, /25→/45, /30→/50, /40→/60), text-foreground (/50→/70, /60→/80, /70→/85), border-border (/10→/18, /12→/20, /15→/25, /20→/35, /30→/45), semantic colors (text-accent/destructive/success/warning /50→/70 and /60→/80), semantic bg (/8→/12, /10→/15), semantic borders (accent/warning/destructive /15→/25, success /20→/30)
- [x] **Fitness charts** — StrainChart: axis labels → CHART_THEME refs, splitLine → CHART_THEME. WellnessScore: radar axis colors aligned to calibrated palette, chart trace #0891b2→#4A80A3
- [x] **TypeScript** — Zero errors (`npx tsc --noEmit` clean)

**Calibration principle**: 3-layer depth system (background #0B0F14 → panels #131921 → primary #161E2A), typography legibility tiers (headings ~#8B95A3, metrics #F0F2F5, secondary 70-85%, tertiary minimum 50%), accent +15% saturation, all borders ≥ /18 opacity, no text below /30 opacity.

## Pending / Future Work

### High Priority
- [ ] **End-to-end Telegram alert test** — Trigger a real health status push from the app and verify the Telegram message arrives with correct formatting
- [ ] **OpenClaw auto-restart** — When health check shows offline, auto-restart via LaunchAgent `launchctl kickstart`; surface result in UI
- [ ] **n8n webhook auth** — The health monitor webhook is currently unauthenticated; add a shared secret header check in the n8n Code node
- [ ] **Whoop BLE stability** — Whoop connection drops intermittently; needs reconnect logic with exponential backoff in `whoop.rs`

### Medium Priority
- [ ] **TokenUsage real data** — `TokenUsage.tsx` uses hardcoded placeholder data; wire to actual token tracking from OpenClaw or n8n API call log
- [ ] **ARM trading live feed** — RiskPanel and PositionsTable are wired but ARM API endpoint URL is not persisted in settings; add to SettingsPage
- [ ] **Neuro session export** — SessionReport dialog generates report but has no share/save-to-disk action; add Tauri `save` dialog
- [ ] **Light mode parity** — `globals.css` has light mode overrides but some newer glass classes (`glass-elevated`, ambient orbs) have no light mode equivalents
- [ ] **Onboarding flow** — OnboardingModal exists but skip/complete state is not persisted; user sees it every launch

### Low Priority / Nice to Have
- [ ] **Trajectory page** — GoalTracker is wired but no backend persistence yet; goals reset on restart
- [ ] **Keyboard shortcuts** — CommandPalette exists but shortcut bindings are sparse; populate with all page navigation + common actions
- [ ] **Assistant page history** — Chat history is in-memory only; persist to SQLite via Tauri plugin
- [ ] **Notification Center** — NotificationCenter dropdown UI exists but doesn't pull from a real notification store
- [ ] **BLE device picker** — Muse and Whoop connect to hardcoded devices; add a scan/pick UI in Settings
- [ ] **Tauri updater** — No auto-update mechanism; add `tauri-plugin-updater` for OTA updates
- [ ] **Production build CI** — No CI/CD; add GitHub Actions for `cargo tauri build` on push

---

## Key Patterns & Conventions

### Adding a New Tauri Command
1. Write `#[tauri::command] pub async fn my_command(...) -> Result<T, String>` in `src-tauri/src/commands/`
2. Register in `mod.rs` and `main.rs` `tauri::Builder::invoke_handler`
3. Add TypeScript wrapper in `src/lib/api.ts` using `invoke<T>('my_command', { args })`
4. Use in components with `useEffect` + `useState`

### Adding a New Page
1. Add nav entry to `Sidebar.tsx` `navItems` array (icon, label, id)
2. Add `case 'my-page':` to App.tsx switch/render block
3. Create `src/components/my-page/` directory with components
4. Use `<div className="space-y-7">` as page root, `<div className="grid grid-cols-2 gap-5">` for 2-col layouts

### Glass Card Pattern
```tsx
<div className="glass p-6 space-y-5">
  <div className="flex items-center justify-between">
    <div className="flex items-center gap-2">
      <IconName size={16} className="text-accent" />
      <h3 className="text-base font-medium text-muted-foreground">Card Title</h3>
    </div>
    {/* optional right slot: badge, status, etc. */}
  </div>
  {/* card content */}
</div>
```

### n8n HTTP Calls from Code Nodes
n8n 2.x task runner sandbox — **no `fetch`, no `process`**. Use:
```js
const response = await helpers.httpRequest({
  url: 'http://host/path',
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ key: value }),
  json: true,
});
```

---

## Important File Locations

| File | Purpose |
|------|---------|
| `src/styles/globals.css` | Entire design system — edit here for global visual changes |
| `src/lib/chart-theme.ts` | ECharts shared constants — edit to change all chart styling |
| `src/lib/types.ts` | All TypeScript interfaces — add new types here |
| `src/App.tsx` | Page routing and top-level state — wire new pages/components here |
| `src-tauri/src/commands/openclaw.rs` | OpenClaw health check logic |
| `src-tauri/src/commands/mod.rs` | Command registration |
| `~/.openclaw/config.json` | OpenClaw runtime config |
| `~/.node-llama-cpp/models/` | Local LLM models for OpenClaw memory search |
| `/opt/homebrew/lib/node_modules/openclaw/` | OpenClaw install location |
| `/Users/thevisualdude/terrance-integrations/stack/integrations/n8n/` | n8n workflow JSON files |
