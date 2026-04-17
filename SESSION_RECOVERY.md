# Session Recovery — Implementation Specification

Status: proposed
Owner: Toph
Audience: the next agent implementing recovery in Sworm

---

## 0. Scope

This document defines **recovery after any interruption**:

- webview reload
- app close and reopen
- app crash / force-kill
- project switch and later return

Recovery means:

- the workspace layout comes back
- the tabs come back
- session tabs come back
- terminal history comes back
- the app accurately shows whether a session is still live or already exited

Recovery does **not** mean re-entering a dead terminal process. If the app process is gone and the PTY was killed or lost, we restore the **terminal transcript**, not the original live shell.

That distinction is load-bearing. Any design that blurs it will produce confusing UX and brittle implementation.

Validation commands that exist today:

```sh
bun run check
bun run build
cargo check
cargo test
bun tauri build --debug
```

Do not invent additional repo-level commands.

---

## 1. Product goal

After any interruption, Sworm should reopen into a state that feels continuous and truthful:

- projects that were open reappear
- panes and tabs are restored
- editor tabs reopen at the same files
- session tabs reopen in the same places
- each terminal shows its prior scrollback/history
- if the underlying PTY is no longer alive, the terminal is visibly historical and not interactive
- if a PTY is still alive in the same backend process, Sworm may resume live streaming after replaying history

The user should never be misled into thinking Sworm restored a live process when it only restored output history.

---

## 2. Non-goals

- Reattaching to dead PTYs after app restart
- Background daemon / broker that outlives the Tauri app
- Full shell checkpoint/restore
- Multi-window synchronization
- Monaco cursor / selection / scroll restoration
- Persisting unsaved editor buffers across crash in V1
- Worktree-per-session

If a future version wants true live-process continuity across app restarts, that requires a separate supervisor architecture. It is not part of this spec.

---

## 3. Current failures

Sworm currently loses recovery state at multiple layers:

1. Backend PTY draining is unsafe on frontend disconnect.
   Current code in [pty.rs](/home/toph/Development/ADE/src-tauri/src/services/pty.rs:149) breaks the reader loop when the output channel drops. That can stop PTY draining and eventually stall the child on PTY writes.

2. Workspace state is memory-only.
   `workspaces`, `openProjectIds`, `activeProjectId`, and `focusedPaneSlot` live only in [workspace.svelte.ts](/home/toph/Development/ADE/src/lib/stores/workspace.svelte.ts:114). Reload, crash, or relaunch drops them.

3. Session tabs are recreated heuristically instead of restored explicitly.
   `syncSessionTabs` auto-creates tabs based on session status, which is a poor substitute for actual workspace persistence.

4. Terminal history is not persisted.
   `TerminalSessionManager` streams live bytes into xterm, but there is no durable transcript.

5. Reload and close flows are unmanaged.
   `Reload View` still calls `window.location.reload()` directly in [view.svelte.ts](/home/toph/Development/ADE/src/lib/commands/view.svelte.ts:42), which bypasses any save/flush flow.

---

## 4. Architecture decisions

These are the intended constraints for implementation.

### 4.1 Recovery is transcript-first, not PTY-first

The persistent source of truth for a terminal is its **saved transcript**.

That transcript is what makes recovery after restart possible.

If a PTY also happens to still be alive in the current process, Sworm can attach to it after replaying the saved transcript. That is an optimization, not the core recovery model.

### 4.2 Workspace persistence is part of normal operation, not a reload hook

Workspace state must be written continuously with a debounce from the main workspace store. Saving only during reload is insufficient because crashes and forced exits bypass that path.

### 4.3 Terminal transcripts live in the backend

Transcript capture belongs in Rust because that is where bytes are read from the PTY. The frontend must not be the only place that sees terminal output.

### 4.4 Session liveness and transcript recovery are different concerns

- `session_is_alive(session_id)` answers whether a PTY is live **right now in this process**
- transcript loading answers what history should be shown after any interruption

Do not overload DB `status` as a proxy for live PTY existence.

### 4.5 Exited sessions still restore as historical terminals

When a session is restored and no PTY is alive, the terminal tab still opens with its saved history. The UI must clearly indicate that it is exited/stopped and input is disabled.

That is correct behavior. It matches the product goal.

### 4.6 No fake resurrection

Sworm must not silently spawn a new PTY just because a terminal tab was restored. Starting a new process is a user action or an explicit session action, not an implicit side effect of viewing history.

---

## 5. Terminal model

The terminal portion of recovery has two modes.

### 5.1 Historical restore

Available after any interruption.

Behavior:

- load the saved transcript for the session
- write it into xterm
- show the session as exited/stopped if no live PTY exists
- keep input disabled

This is the default recovery path after app restart.

### 5.2 Live continuation

Available only when the backend PTY still exists in the current app process, for example after a webview reload.

Behavior:

- load and render saved transcript first
- then attach live output channels
- then enable input

This is a narrower case. It must not shape the overall architecture more than necessary.

---

## 6. Data model

We need persistence in three areas:

1. workspace layout per project
2. app shell state
3. terminal transcript per session

### 6.1 Workspace layout

Add a new table:

```sql
CREATE TABLE workspace_state (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  state_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

This stores the per-project tab/pane layout.

### 6.2 App shell state

Add a new table:

```sql
CREATE TABLE app_state (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

This stores:

- open project ids
- active project id

Keep this separate from `app_settings`. Preferences and hot restore state are different classes of data.

### 6.3 Terminal transcript

Reuse the existing `session_entries` table from [V1__initial.sql](/home/toph/Development/ADE/src-tauri/migrations/V1__initial.sql:13).

Use it for terminal output entries:

- `kind = 'output'`
- `payload_json = {"chunk":"<base64>", "seq": N}`

Design notes:

- Use append-only rows during session runtime
- Keep a monotonic `seq` per session so replay order is deterministic
- Base64 is preferred over JSON number arrays for size and parsing cost

We are not storing terminal history inside the workspace blob. Keep identity/layout data and transcript data separate.

---

## 7. Persisted workspace shape

Persist a versioned JSON shape.

```ts
export interface PersistedWorkspaceV1 {
  version: 1
  focusedPaneSlot: PaneSlot
  splitMode: SplitMode
  quadLayout: QuadLayout
  panes: Array<{
    slot: PaneSlot
    activeTabIndex: number
    tabIndices: number[]
  }>
  tabs: PersistedTab[]
}

export type PersistedTab =
  | { kind: 'session'; sessionId: string; title: string; providerId: string; locked: boolean }
  | { kind: 'editor'; filePath: string; gitRef?: string; refLabel?: string; temporary: boolean; locked: boolean }
  | { kind: 'commit'; commitHash: string; shortHash: string; message: string; initialFile: string | null; temporary: boolean; locked: boolean }
  | { kind: 'changes'; staged: boolean; initialFile: string | null; temporary: boolean; locked: boolean }
  | { kind: 'stash'; stashIndex: number; message: string; initialFile: string | null; temporary: boolean; locked: boolean }
  | { kind: 'notification-test'; label: string; temporary: boolean; locked: boolean }
```

Rules:

- tab ids are not persisted
- pane/tab relationships are persisted by index
- restore generates fresh ephemeral tab ids
- `dirty` editor state is not persisted in V1

---

## 8. Backend changes

### 8.1 Phase A: fix PTY drain correctness

File: [pty.rs](/home/toph/Development/ADE/src-tauri/src/services/pty.rs:149)

Requirement:

- output channel drop must not break PTY draining
- reader thread must continue reading until child exit or shutdown

This is mandatory even before transcript persistence, otherwise long-running shells can still wedge after frontend loss.

Also replace the current `finalized_ptys` set in [pty.rs](/home/toph/Development/ADE/src-tauri/src/services/pty.rs:39) with a per-`LivePty` finalization guard such as `Arc<AtomicBool>`. The current global set is unnecessary complexity and leaks entries.

### 8.2 Phase B: add in-memory ring buffer

Add a bounded in-memory ring buffer on `LivePty`.

Purpose:

- survive frontend reload inside the same process
- coalesce recent bytes before disk flush

Cap:

- default `1 MiB` per live session

This ring buffer is not the durable source of truth for restart recovery. It is an in-process cache.

### 8.3 Phase C: append transcript entries to disk

As the PTY reader reads bytes:

1. append bytes to in-memory ring
2. append bytes to a per-session pending buffer
3. flush buffered output to `session_entries` on either threshold:
   - `>= 16 KiB` buffered, or
   - `>= 500 ms` since last flush

Also flush on:

- PTY exit
- session stop
- app shutdown

Implementation note:

- keep DB writes off the hot byte-by-byte path
- coalesce chunks before insert
- preserve order with a monotonic `seq`

### 8.4 Phase D: transcript query commands

Add commands in `src-tauri/src/commands/sessions.rs` or a dedicated transcript command file:

```rust
#[tauri::command]
pub fn session_transcript_get(
    session_id: String,
    limit_bytes: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError>
```

Return value:

- base64-encoded concatenated transcript bytes

Behavior:

- load transcript rows in `seq` order
- concatenate
- if `limit_bytes` is set, return only the tail window

V1 should default to restoring the last `1 MiB`.

### 8.5 Phase E: optional live attach

Keep or add:

```rust
#[tauri::command]
pub fn session_is_alive(...)

#[tauri::command]
pub fn session_attach(...)
```

But treat them as optional live continuation helpers.

They are not required for restart recovery. They are only required if we also want reload-without-process-loss to continue live.

### 8.6 Phase F: workspace/app state services

Add:

- `src-tauri/src/services/workspace_state.rs`
- `src-tauri/src/commands/workspace.rs`

Commands:

```rust
workspace_state_get(project_id)
workspace_state_put(project_id, state_json)
app_state_get(key)
app_state_put(key, value_json)
```

This should follow the repo’s existing service/command split.

---

## 9. Frontend changes

### 9.1 Persist on workspace mutation

File: [workspace.svelte.ts](/home/toph/Development/ADE/src/lib/stores/workspace.svelte.ts:151)

`commitWorkspace` is the correct choke point. Keep using it.

Add debounced persistence there:

- serialize workspace state
- write to backend after `250 ms`
- coalesce rapid tab/pane changes

Also add a debounced persist path for app shell state:

- `openProjectIds`
- `activeProjectId`

Do not wait for reload or window close.

### 9.2 Restore on app start

App startup flow should become:

1. read `app_state` for open project ids
2. restore those projects as open
3. for each project, load persisted workspace layout
4. then load sessions from DB
5. reconcile restored session tabs against actual sessions

Important:

- restored workspace tabs are the source of truth
- `syncSessionTabs` becomes a reconciler, not a tab creator

### 9.3 Remove session-tab recreation heuristic as primary behavior

Current `ACTIVE_STATUSES` in [workspace.svelte.ts](/home/toph/Development/ADE/src/lib/stores/workspace.svelte.ts:15) should no longer drive session-tab creation during normal restore.

Instead:

- if persisted workspace exists, restore from it
- then reconcile against session DB rows

A limited bootstrap heuristic is acceptable only for users upgrading from pre-recovery state with no saved workspace blob yet.

### 9.4 Terminal mount flow

`TerminalSessionManager` should be split conceptually into:

- `loadTranscript()`
- `attachLivePty()` if alive
- `startPty()` only on explicit start action

Mount behavior for an existing session tab:

1. create xterm
2. fetch transcript
3. write transcript and wait for xterm flush callback
4. check `session_is_alive`
5. if alive, attach live channels and enable input
6. if not alive, keep input disabled and show historical/exited state

This ordering matters. Transcript must render before live attach to avoid output reordering.

### 9.5 Do not auto-spawn on restored tab mount

Current session terminal logic in [SessionTerminal.svelte](/home/toph/Development/ADE/src/lib/components/session/SessionTerminal.svelte:73) auto-starts if the manager is inactive.

That is wrong for recovery.

Restoring a tab must not create a new process implicitly. A restored historical terminal is still a successful recovery result.

### 9.6 Historical terminal UX

When transcript is shown without a live PTY:

- disable input
- show a small banner or status line: `Session exited. Showing restored terminal history.`
- keep scrollback and selection usable

This avoids the common confusion of “why won’t this terminal accept input?”

---

## 10. Recovery semantics by interruption type

### 10.1 Webview reload, backend still alive

Expected result:

- workspace restored from persisted state or still-live memory
- transcript restored
- if PTY alive, live attach resumes

### 10.2 Clean app close and reopen

Current Sworm behavior in [lib.rs](/home/toph/Development/ADE/src-tauri/src/lib.rs:134) kills PTYs on app exit.

Expected result:

- workspace restored
- transcript restored
- sessions shown as stopped/exited
- no live terminal input

### 10.3 Crash / force-kill and reopen

Expected result:

- workspace restored from last debounced persisted state
- transcript restored up to last successful flush
- startup reconciliation marks stale running sessions exited via existing startup logic in [app_state.rs](/home/toph/Development/ADE/src-tauri/src/app_state.rs:49)

This is why transcript flush cadence matters. A crash can lose only the most recent unflushed tail.

### 10.4 Project close and later reopen

Expected result:

- closing a project removes it from the open-project app state
- reopening it later restores the project workspace from `workspace_state`

Project-close should not delete persisted workspace unless explicitly requested by product design. Default recommendation: keep it.

---

## 11. UX requirements

### 11.1 Reload must be managed

Replace direct `window.location.reload()` in [view.svelte.ts](/home/toph/Development/ADE/src/lib/commands/view.svelte.ts:42) with:

1. dirty-tab check
2. confirm if needed
3. force-flush pending workspace persists
4. then reload

### 11.2 Dirty editor protection

Track dirty state on `EditorTab` in the workspace store so tab-close and reload can reason about it centrally.

Do not persist dirty state across crash in V1. After restart, reopen the file tab but not the unsaved buffer contents.

### 11.3 Truthful session status

Session tabs should distinguish at least:

- running and live
- exited/stopped with transcript available
- failed

The recovered historical terminal should not look identical to a live terminal.

### 11.4 No fake “detached but recoverable” state after restart

After app restart, there is no in-memory PTY map. Do not invent a special detached status that implies the terminal might still be resumed. Show the restored history and the persisted session status after reconciliation.

---

## 12. What is intentionally removed from the old plan

The following ideas are removed or demoted because they were distorting the real goal:

1. `session_attach` as the core recovery story
   It is not. Transcript recovery is the core story.

2. `session_is_alive` as a primary restore decision
   It is only relevant for same-process live continuation.

3. Promise of “quit/relaunch continues the old live terminal”
   That is not realistic with the current architecture.

4. `bind_error` as a core recovery feature
   It is orthogonal. If wanted, it can be added later as session UX polish. It is not needed for this project to succeed.

---

## 13. Implementation phases

These phases are sequential and landable.

### Phase 1: backend correctness

Deliverables:

- PTY reader keeps draining after frontend disconnect
- replace `finalized_ptys` with per-session finalization guard

Exit criteria:

- webview reload does not wedge a noisy shell
- `cargo test` covers the finalization path

### Phase 2: transcript capture and replay

Deliverables:

- in-memory ring buffer on live PTYs
- transcript flush to `session_entries`
- transcript retrieval command
- frontend transcript replay into xterm
- historical terminal mode

Exit criteria:

- start a session, produce large output, close app, reopen app, terminal history is restored
- after restart, restored terminal is visibly non-interactive if the session is dead
- crash loses at most the latest unflushed tail, not all history

### Phase 3: workspace and app-shell persistence

Deliverables:

- `workspace_state` table
- `app_state` table
- serialize/hydrate workspace
- restore open projects, active project, panes, and tabs
- convert `syncSessionTabs` into reconciliation logic

Exit criteria:

- reopen app after interruption and recover the same project/tab layout
- session tabs restore in place instead of being heuristically recreated
- deleted or invalid targets fail gracefully

### Phase 4: safety and UX polish

Deliverables:

- managed reload flow
- dirty editor tracking
- close/reload confirms
- truthful live-vs-historical session presentation
- optional same-process live attach after transcript replay

Exit criteria:

- reload warns on dirty tabs
- restored historical terminals are clear and usable
- same-process reload can continue live output without reordering transcript and live bytes

---

## 14. File map

Primary files likely touched by this spec:

- [src-tauri/src/services/pty.rs](/home/toph/Development/ADE/src-tauri/src/services/pty.rs)
- [src-tauri/src/services/sessions.rs](/home/toph/Development/ADE/src-tauri/src/services/sessions.rs)
- `src-tauri/src/services/workspace_state.rs` new
- [src-tauri/src/commands/sessions.rs](/home/toph/Development/ADE/src-tauri/src/commands/sessions.rs)
- `src-tauri/src/commands/workspace.rs` new
- [src-tauri/src/app_state.rs](/home/toph/Development/ADE/src-tauri/src/app_state.rs)
- [src-tauri/src/lib.rs](/home/toph/Development/ADE/src-tauri/src/lib.rs)
- [src-tauri/migrations/V1__initial.sql](/home/toph/Development/ADE/src-tauri/migrations/V1__initial.sql)
- `src-tauri/migrations/V5__workspace_state.sql` new
- `src-tauri/migrations/V6__app_state.sql` new
- [src/lib/api/backend.ts](/home/toph/Development/ADE/src/lib/api/backend.ts)
- [src/lib/stores/workspace.svelte.ts](/home/toph/Development/ADE/src/lib/stores/workspace.svelte.ts)
- [src/lib/stores/sessions.svelte.ts](/home/toph/Development/ADE/src/lib/stores/sessions.svelte.ts)
- [src/lib/terminal/TerminalSessionManager.ts](/home/toph/Development/ADE/src/lib/terminal/TerminalSessionManager.ts)
- [src/lib/components/session/SessionTerminal.svelte](/home/toph/Development/ADE/src/lib/components/session/SessionTerminal.svelte)
- [src/lib/components/editor/FileEditor.svelte](/home/toph/Development/ADE/src/lib/components/editor/FileEditor.svelte)
- [src/lib/commands/view.svelte.ts](/home/toph/Development/ADE/src/lib/commands/view.svelte.ts)

---

## 15. Pitfalls

### 15.1 xterm ordering

Transcript replay must complete before live attach starts streaming. Use xterm’s write callback to enforce ordering.

### 15.2 Debounce versus crash loss

Workspace and transcript persistence are both debounced. That is correct, but it means a crash can lose the most recent tail. Keep the windows short and explicit.

### 15.3 DB write amplification

Do not write transcript rows for every small chunk. Batch them.

Do not persist workspace state on every keystroke from editor dirty updates if the serialized blob is unchanged.

### 15.4 Session removal and transcript GC

If a session is deleted, its transcript rows in `session_entries` should also be deleted via FK cascade or explicit cleanup.

### 15.5 Restored invalid tabs

Files may be deleted, stash indices may change, sessions may be removed. Restore should be resilient and then reconcile, not fail the entire workspace.

---

## 16. Final posture

This plan is correct for Sworm if we hold these lines:

- recovery means restoring **state and history**
- not resurrecting dead processes
- transcript is durable, PTY liveness is ephemeral
- restored terminals may be historical-only, and that is a first-class success case
- workspace persistence happens continuously, not only during polite shutdown paths

If implementation starts to optimize same-process live attach at the expense of cross-interruption transcript recovery, it is going in the wrong direction.
