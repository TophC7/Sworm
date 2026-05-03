---
name: debug-rootcause
description: >
  Evidence-first debugging loop for bugs that resist a first fix. Use when a bug has already
  survived one attempted patch, when the user says "still broken", "you keep guessing",
  "stop making assumptions", "shotgun", or explicitly asks to debug/root-cause an issue.
  Also trigger on UI/focus/rendering/lifecycle bugs, Svelte `$effect` misbehavior, Tauri
  command wiring failures, and anything where the failure mode is not directly visible in
  the diff. Do NOT use for trivial typos, compile errors, or bugs where the cause is already
  stated in the stack trace.
---

# Debug Root Cause

Senior debugger. Goal: **prove the cause before proposing a fix.** No speculative patches.

This skill exists because shotgun-patching wastes turns and erodes trust. The loop below is
structurally enforced — you do not skip steps even if you feel confident.

## Iron Laws

1. **NO FIX WITHOUT EVIDENCE** — Do not edit production code until a hypothesis is confirmed by observed output, a code-path trace, or a repro.
2. **ONE HYPOTHESIS AT A TIME** — Rank 2-3 candidates, pick the strongest, test it. Do not instrument everything at once.
3. **INSTRUMENT, DON'T GUESS** — When unsure, add logs. `console.log`, `eprintln!`, `dbg!`, Svelte `$inspect`. Temporary by contract.
4. **REMOVE YOUR TRACES** — Every log, `$inspect`, or `dbg!` you added gets deleted before you declare done. Leave no debug residue.
5. **LOCK IT IN** — A confirmed root cause deserves a regression test or a code-adjacent comment so it does not come back.

## When To Invoke

Use this skill proactively when any of these are true:

- You already tried one fix and the bug persists.
- The user says "still broken", "dude", "stop", "you keep guessing", or otherwise signals frustration.
- The failure is in focus, rendering, lifecycle, async ordering, IPC wiring, or process spawning — anything where reading the diff is not enough.
- You do not have a one-sentence explanation of the failure mode.

If you catch yourself typing an `Edit` call without being able to state *exactly which line produces the wrong behavior and why*, stop and enter this loop.

## The Loop

### Phase 1 — Reproduce and Restate

1. Restate the bug in one sentence: **what happens vs. what should happen**.
2. Confirm you can reproduce it, or ask the user for the exact steps. No repro → no fix.
3. List the smallest set of files involved. Read them end-to-end, not just the diff.

Do not proceed until you can say out loud what the user sees and where in the code it must originate.

### Phase 2 — Hypothesize

1. List 2-3 candidate root causes. Each candidate names a specific line, function, or lifecycle event.
2. Rank them by likelihood with a one-line reason.
3. Pick the top candidate. State what observation would confirm it and what would falsify it.

A hypothesis like "probably a race condition" is not acceptable. "`TabBar.svelte:84` fires `$effect` before `mount`, so `editor` is `undefined` when height is read" is acceptable.

### Phase 3 — Instrument

1. Add temporary diagnostics targeted at the top hypothesis only.
   - Svelte: `$inspect(value)`, `console.log('[debug focus]', ...)` with a greppable prefix.
   - Rust: `eprintln!("[debug cmd] {:?}", ...)` or `tracing::debug!`. Never `println!` in Tauri commands — use `eprintln!` so it reaches the dev console.
   - Tauri IPC: log on both sides of `invoke` to confirm the call crossed.
2. Use a consistent prefix (`[debug-<topic>]`) so the traces are trivially greppable at cleanup.
3. Tell the user exactly how to reproduce and what output to paste back.

Do not instrument every candidate at once. One hypothesis, one set of probes.

### Phase 4 — Observe

1. Run the repro (or wait for the user's paste).
2. Compare observed output to the falsification criterion from Phase 2.
3. If the hypothesis is **confirmed** → Phase 5.
4. If **falsified** → discard it, move to the next ranked candidate, return to Phase 3. Do not patch "just in case."
5. If output is ambiguous → add more targeted probes, do not guess.

### Phase 5 — Fix

1. Write the **smallest** change that addresses the confirmed cause. Resist fixing adjacent smells in the same edit.
2. State in one sentence why this edit eliminates the observed failure.
3. Re-run the repro. Confirm the observed output is now correct.
4. Check the sibling cases that live near this bug (see Phase 6).

### Phase 6 — Verify Sibling Cases

For any UI / lifecycle / IPC fix, before declaring done, explicitly enumerate adjacent states:

- **Tabs / panes**: fresh workspace, empty state, single tab, multi-tab, split pane, drag-reordered.
- **Focus / modal**: dialog open, dialog nested, dialog dismissed via Esc vs. backdrop, ConfirmDialog path.
- **Project-scoped commands**: no project open, project open but no session, multiple projects.
- **Async / IPC**: success path, backend error, timeout, rapid repeat invocation.

State what you verified for each. "Only tested the happy path" is a valid answer — say it explicitly instead of silently skipping.

### Phase 7 — Lock and Clean

1. Remove every temporary log, `$inspect`, `dbg!`, and debug-only branch you added.
   - Grep for your prefix (`[debug-<topic>]`) to confirm zero hits.
2. If the project has a test surface for this area, add a regression test that fails before the fix and passes after.
3. If a test is not practical (pure UI / timing), leave a single `// CLAUDE:` or short comment near the fix explaining the invariant, following `.claude/comment-style/SKILL.md`.
4. Run the narrowest validation commands that apply:
   - Frontend: `bun run check`
   - Rust: `cargo check`
   - Integration: `cargo test` if a test was added.

## Sworm-Specific Hotspots

Based on prior failure modes in this repo, watch these categories first:

| Symptom | First hypothesis to test |
|---|---|
| Focus doesn't restore after a dialog closes | Phantom `DialogContent` still registered in the stack — grep for active registrations. |
| UI element sized to zero or wrong height | `$effect` fires before child mounts; read happens against `undefined`. Instrument mount order. |
| Tab behavior regresses only on fresh workspace | State initialized from an empty store, check the "no project / no session" branch. |
| Tauri command "does nothing" | `invoke` never crossed — `eprintln!` on Rust entry before blaming the handler. |
| Command operates on wrong directory | Project path not passed; confirm CWD at the Rust boundary, not the frontend. |
| Svelte value looks stale | `$state` being mutated via reassignment of a derived, or `$effect` mirroring state that should be `$derived`. |

## Anti-Patterns

Stop immediately if you notice yourself:

- Making a second speculative fix without having run any diagnostic.
- Editing a file you have not read end-to-end in this session.
- Describing the fix before describing the observed failure mode.
- Saying "let me try" anything. Trying is Phase 3 instrumentation, not an edit.
- Leaving debug logs in because "they might be useful later." Delete them. If they are useful later, the `tracing` crate or a proper logger is the answer.

## Output Contract

When reporting progress, follow this structure so the user can scan it:

```md
## Bug
One sentence: observed vs. expected.

## Hypotheses
1. <top candidate> — confirm by: <observation>, falsify by: <observation>
2. <second> — ...
3. <third> — ...

## Instrumentation Added
- `path:line` — `[debug-<topic>]` log of <what>
- ...

## Observed
<paste or describe the output>

## Root Cause
One sentence naming the exact line and failure mode.

## Fix
- `path:line` — <one-line description>
- Sibling cases verified: <list>
- Diagnostics removed: yes
- Regression locked in: test at `path` / comment at `path:line` / N/A because <reason>
```

If you cannot fill in "Root Cause" with a specific line, do not move to "Fix". Go back to Phase 3.
