# Plan: voice-type — Rebrand + Profiling Dashboard

## Context

Building a portfolio piece for a job application to The Call Center Doctors (AI-Augmented Full Stack Developer). The omarchy-voice-typing project — a production Rust+React voice-to-text gateway — gets rebranded as **"voice-type"** and enhanced with a deep profiling dashboard. Voice session analytics is directly relevant to their call center business.

**Current state:** Rust gateway (47 tests), React+TS frontend (recordings list, audio playback, transcript edit), 361 recordings, 303 session logs, 180 latency records in JSONL.

**Target state:** A polished product with two pages — Recordings (existing, restructured) and Profiling (new — session explorer with editable transcripts and UML-style sequence diagrams showing event flow between components). Frontend tests. Hostable on prabhanshu.space.

---

## Phase 1: Backend — Profiling API Endpoints

Add 3 new endpoints to the Rust gateway that serve the existing log data as structured JSON.

### 1.1 New file: `gateway-rs/src/handlers/profiling.rs`

**`GET /api/profiling/latency?from=YYYY-MM-DD&to=YYYY-MM-DD`**
- Read all `latency_*.jsonl` files from log dir
- Parse each line with `serde_json::from_str` into `LatencyRecord`
- Filter by date range, sort by `timestamp_unix` ascending
- Return `Json<Vec<LatencyRecord>>`

**`GET /api/profiling/sessions?from=YYYY-MM-DD&to=YYYY-MM-DD&limit=50`**
- Read `.log` files from sessions dir
- Parse the human-readable format (line scanner on known prefixes: `Started:`, `Duration:`, `Deepgram connect:`, `Status:`, `Transcript:`, timeline entries)
- Return `Json<Vec<SessionSummary>>`

**`GET /api/profiling/summary?from=YYYY-MM-DD&to=YYYY-MM-DD`**
- Aggregate latency records: total, success/failure count, success rate
- Compute percentiles (P50/P95/P99) for `total_commit_ms` and `transcription_ms`
- Group by backend → per-backend avg/percentiles
- Group by date → daily counts
- Return `Json<ProfilingSummary>`

### 1.2 Modify: `gateway-rs/src/state.rs`
- Add `latency_log_dir: PathBuf` and `session_log_dir: PathBuf` fields

### 1.3 Modify: `gateway-rs/src/handlers/mod.rs`
- Add `pub mod profiling;`

### 1.4 Modify: `gateway-rs/src/lib.rs`
- Wire 3 new routes under `/api/profiling/*`

### 1.5 Tests
- Unit tests in `profiling.rs` for JSONL parsing, session log parsing, percentile calculation
- E2E tests in `e2e_test.rs` using temp log files

---

## Phase 2: Frontend Restructure

Break the monolithic `App.tsx` (329 lines) into a proper component architecture with React Router.

### 2.1 Install dependencies
```
react-router ^7.5.0
recharts ^2.15.0
vitest ^3.2.0 (dev)
@testing-library/react ^16.3.0 (dev)
@testing-library/jest-dom ^6.6.0 (dev)
@testing-library/user-event ^14.6.0 (dev)
jsdom ^26.0.0 (dev)
```

### 2.2 New directory structure
```
web/src/
├── main.tsx                          # Add BrowserRouter
├── router.tsx                        # NEW — Routes: / and /profiling
├── index.css                         # KEEP
├── lib/
│   ├── api.ts                        # MOVED from src/ + 3 new profiling fetchers
│   ├── types.ts                      # MOVED from src/ + profiling types
│   ├── utils.ts                      # NEW — extracted formatBytes/Date/Duration
│   ├── chartTheme.ts                 # NEW — shared Recharts color/style constants
│   └── hooks/
│       ├── useLatencyData.ts         # NEW
│       ├── useSessionData.ts         # NEW
│       └── useProfilingSummary.ts    # NEW
├── components/
│   └── layout/
│       ├── Shell.tsx + Shell.css     # NEW — header + nav + <Outlet />
│       ├── Nav.tsx                   # NEW — Recordings | Profiling links
│       └── StatsBar.tsx              # NEW — extracted stats bar
├── pages/
│   ├── recordings/
│   │   ├── RecordingsPage.tsx + .css # Extracted from App.tsx
│   │   ├── EntryCard.tsx             # Extracted from App.tsx
│   │   └── DetailPanel.tsx           # Extracted from App.tsx
│   └── profiling/
│       ├── ProfilingPage.tsx + .css  # NEW — session explorer layout
│       ├── SessionList.tsx           # NEW — left panel session log
│       ├── SessionDetail.tsx         # NEW — right panel container
│       ├── TranscriptEditor.tsx      # NEW — editable transcript + save
│       ├── SpeedProfile.tsx          # NEW — horizontal timing bars
│       └── sequence-diagram/         # NEW — SVG sequence diagram
│           ├── SequenceDiagram.tsx
│           ├── Participant.tsx
│           ├── Arrow.tsx
│           ├── TimeMarker.tsx
│           └── types.ts
└── test/
    ├── setup.ts                      # vitest + RTL setup
    └── __tests__/                    # 6 test files
```

### 2.3 DELETE after extraction
- `web/src/App.tsx` → replaced by Shell + pages
- `web/src/App.css` → split into component CSS files

---

## Phase 3: Profiling Page — Session Explorer

The profiling page is a **session explorer** — not a generic charts dashboard. It combines a recording log, editable transcripts (training feedback), and a UML-style sequence diagram showing the event timeline between system components.

### 3.1 Layout: Three-panel view on `/profiling`

```
+------------------+-----------------------------------------------+
| SESSION LOG      | DETAIL VIEW                                   |
| (scrollable      |                                               |
|  list)           | [Audio Player ▶ ━━━━━━━━━━ 6.2s]              |
|                  |                                               |
| ● rec-003  OK   | TRANSCRIPT (editable)                         |
|   6.2s deepgram  | ┌─────────────────────────────────────────┐   |
|                  | │ It must be on one of the external drives │   |
| ● rec-012  OK   | │ at attached.                              │   |
|   44.5s whisper  | └─────────────────────────────────────────┘   |
|                  | [Save Correction]  (feeds training data)      |
| ● rec-016  OK   |                                               |
|   14.8s whisper  | SEQUENCE DIAGRAM                              |
|                  | ┌──────────┐  ┌──────────┐  ┌──────────┐     |
| ● rec-004 FAIL  | │hyprwhspr │  │ Gateway  │  │ Deepgram │     |
|   2.1s deepgram  | └────┬─────┘  └────┬─────┘  └────┬─────┘     |
|                  |      │             │              │           |
| (303 sessions)   |      │──audio───>│  T+0.0s      │           |
|                  |      │             │──connect──>│  T+0.0s   |
|                  |      │             │<─connected──│  T+1.7s   |
|                  |      │             │  (1706ms)   │           |
|                  |      │──commit──>│  T+4.3s      │           |
|                  |      │             │──finalize─>│            |
|                  |      │             │<─transcript─│  T+6.2s   |
|                  |      │<─result────│  (1926ms)   │           |
|                  |                                               |
|                  | SPEED PROFILE                                 |
|                  | DG connect:  ████████████░░░  1706ms          |
|                  | Transcribe:  ██████████░░░░░  1926ms          |
|                  | Total:       █████████████████ 6213ms         |
+------------------+-----------------------------------------------+
```

### 3.2 Left panel: Session log list

- Shows all 303+ sessions, newest first
- Each entry shows: session ID, duration, backend (deepgram/whisper), status (OK/FAIL)
- Color-coded: green dot for OK, red for FAIL, orange for offline path
- Filterable by: date range, backend, status
- Click to select → loads detail view

### 3.3 Right panel top: Audio + Editable Transcript

- Audio player (same as current recordings page)
- Transcript text in an editable textarea
- **"Save Correction" button** — saves via `PUT /api/transcript/:filename`
- The correction serves as training feedback (ground truth vs what the model produced)
- Shows original backend that produced the transcript (badge: "deepgram" / "local-whisper")

### 3.4 Right panel center: Sequence Diagram (the main feature)

A **UML sequence diagram** rendered in SVG/Canvas showing the event flow between participants.

**Participants (columns):**

| Column | Maps to log layer | Color |
|--------|-------------------|-------|
| `hyprwhspr` | `PYTHON` / user toggle events | `#9277ff` (light purple) |
| `Gateway` | `GATEWAY` events | `#7c5dfa` (accent purple) |
| `Deepgram` | `DEEPGRAM` events | `#ff8f00` (orange) |
| `Local Whisper` | shown when offline path | `#33d69f` (green) |

**Arrow types:**

| Arrow | Event | Label |
|-------|-------|-------|
| `hyprwhspr ──>  Gateway` | `first audio chunk` | "audio (3072 bytes)" |
| `Gateway ──> Deepgram` | `connected` | "connect" |
| `Deepgram ──> Gateway` | `connected in Xs` | "connected (1706ms)" |
| `hyprwhspr ──> Gateway` | `commit received` | "commit" |
| `Gateway ──> Deepgram` | `taking ONLINE path` | "finalize" |
| `Deepgram ──> Gateway` | `commit complete` | "transcript (53 chars)" |
| `Gateway ──> hyprwhspr` | result delivery | "result" |

**For offline sessions:** The Deepgram column is replaced/joined by Local Whisper:
| `Gateway ──> Local Whisper` | `taking OFFLINE path` | "transcribe WAV" |
| `Local Whisper ──> Gateway` | `commit complete` | "transcript (514 chars, 3821ms)" |

**Timing annotations:**
- Each arrow has a timestamp on the left margin (`T+0.0s`, `T+1.7s`, etc.)
- Latency labels on return arrows (e.g., "1706ms")
- Vertical spacing proportional to elapsed time (so long waits look long)

**Implementation approach:**
- Pure SVG rendered by React (no external diagramming library)
- Components: `SequenceDiagram.tsx`, `Participant.tsx`, `Arrow.tsx`, `TimeMarker.tsx`
- Data: parsed from the session timeline events
- The DATA FLOW section maps to a status row at the bottom (OK/OK/OK/OK or with failures highlighted)

### 3.5 Right panel bottom: Speed Profile bars

Horizontal bar chart showing the timing breakdown:
- Deepgram connect: `████████░░░░` 1706ms
- Transcription: `██████░░░░░░` 1926ms
- Total: `████████████` 6213ms

Simple CSS bars (no charting library needed). Color-coded by component.

### 3.6 Summary stats (top of page or sidebar header)

Quick KPIs from the summary endpoint:
- Total sessions | Success rate | P50 latency | P95 latency | Backend split

### 3.7 Component files

```
pages/profiling/
├── ProfilingPage.tsx          # Three-panel layout + data fetching
├── ProfilingPage.css          # Grid layout styles
├── SessionList.tsx            # Left panel: scrollable session log
├── SessionDetail.tsx          # Right panel: audio + transcript + diagram
├── TranscriptEditor.tsx       # Editable transcript with save
├── SpeedProfile.tsx           # Horizontal timing bars
└── sequence-diagram/
    ├── SequenceDiagram.tsx    # SVG container, maps events to arrows
    ├── Participant.tsx        # Column header box (hyprwhspr, Gateway, etc.)
    ├── Arrow.tsx              # Horizontal arrow with label and latency
    ├── TimeMarker.tsx         # Left-margin timestamp (T+0.0s)
    └── types.ts               # DiagramEvent, Participant types
```

---

## Phase 4: Rebrand to "voice-type"

| File | Change |
|------|--------|
| `web/index.html` | `<title>voice-type</title>` |
| `Shell.tsx` | Logo text → "voice-type" |
| `gateway-rs/Cargo.toml` | `name = "voice-type"` |
| `README.md` | Full rewrite with product framing |

---

## Phase 5: Frontend Tests

**Framework:** Vitest + React Testing Library + jsdom

| Test File | Covers |
|-----------|--------|
| `utils.test.ts` | formatBytes, formatDate, formatDuration, formatMs |
| `api.test.ts` | URL construction, error handling, fetch mocking |
| `EntryCard.test.tsx` | Renders badges, timestamp, preview text, click handler |
| `DetailPanel.test.tsx` | Audio player, transcript display, edit mode, save |
| `StatsBar.test.tsx` | Renders stat values correctly |
| `ProfilingPage.test.tsx` | Loading state, renders charts after data loads |

---

## Phase 6: Build & Hosting

- `npm run build` → `web/dist/` (already works)
- `cargo build --release` → binary serves SPA + API
- Deploy to VPS, nginx reverse-proxy at a subpath on prabhanshu.space
- Or Docker image for portable demo

---

## Implementation Order

| Step | What | Depends on |
|------|------|-----------|
| 1 | Backend: `profiling.rs` + wire routes + state changes | — |
| 2 | Backend: tests for profiling endpoints | Step 1 |
| 3 | Frontend: install deps, create dir structure | — (parallel with 1) |
| 4 | Frontend: extract Shell, Nav, StatsBar, router | Step 3 |
| 5 | Frontend: extract RecordingsPage, EntryCard, DetailPanel | Step 4 |
| 6 | Frontend: lib/ (api, types, utils, chartTheme, hooks) | Step 4 |
| 7 | Frontend: ProfilingPage + SessionList + SessionDetail + TranscriptEditor | Step 6 |
| 7b | Frontend: SVG sequence diagram (SequenceDiagram, Participant, Arrow, TimeMarker) | Step 7 |
| 7c | Frontend: SpeedProfile bars + summary KPIs | Step 7 |
| 8 | Frontend: tests | Step 5, 7 |
| 9 | Rebrand | Step 5 |
| 10 | Build, verify, deploy config | Step 7, 9 |

---

## Verification

1. `cd gateway-rs && cargo test` — all 47+ existing tests pass, new profiling tests pass
2. `cd web && npm run build` — clean build, no TS errors
3. `cd web && npm test` — all frontend tests pass
4. Start gateway, open browser:
   - `/` shows recordings page (same functionality as before)
   - `/profiling` shows session explorer with session list on left
   - Click a session → sequence diagram renders with correct participants + arrows
   - Edit transcript → save → verify PUT succeeds
   - Speed profile bars show correct ms values
   - Offline sessions show Local Whisper column instead of Deepgram
5. `curl localhost:8765/api/profiling/summary` returns valid JSON
6. `curl localhost:8765/api/profiling/latency` returns array of records

---

## Key Files Reference

| Purpose | Path |
|---------|------|
| Rust handler pattern to follow | `gateway-rs/src/handlers/web.rs` |
| Latency JSONL schema | `gateway-rs/src/logging/latency.rs` |
| Session log format | `gateway-rs/src/logging/session_log.rs` |
| Router wiring | `gateway-rs/src/lib.rs` |
| Current monolithic frontend | `web/src/App.tsx` |
| CSS theme variables | `web/src/App.css` (lines 1-30) |
| Sample latency data | `logs/latency/latency_2026-02-20.jsonl` (60 records) |
| Sample session log | `logs/sessions/20260226_190639_rec-003.log` |
