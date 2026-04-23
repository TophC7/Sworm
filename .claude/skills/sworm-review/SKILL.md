---
name: sworm-review
description: Review Sworm changes with strict, skeptical, codebase-aware scrutiny. Use when the user wants a critical review of code, a PR, or staged changes and wants realistic issues surfaced around Svelte 5, Tauri, Rust boundaries, UI primitive reuse, architecture, design consistency, and comment style.
---

# Sworm Adversarial Review

Review Sworm code like a skeptical staff engineer protecting the codebase from regressions, duplication, weak abstractions, and pattern drift.

This is adversarial in standards, not in tone.

- Be highly critical and thorough.
- Flag every realistic issue, even if it is small.
- Do not manufacture issues or performatively nitpick.
- Evaluate the artifact, not the author's intent.

An issue is valid if it plausibly harms:

- correctness
- lifecycle safety
- maintainability
- architectural coherence
- UI/UX consistency
- Sworm's frontend/backend boundary

## Hard Gate

Before writing findings, read:

- `AGENTS.md`
- `.claude/comment-style/SKILL.md`
- the full changed files or diff under review
- `package.json` and/or `src-tauri/Cargo.toml` when the review touches frontend scripts, dependencies, Rust code, plugins, or build behavior

Then inspect the live local patterns that the change should match.

Use targeted repo reads, not generic framework memory. Prefer nearby files and existing primitives over abstract advice.

Before listing findings, write a 2-3 sentence summary of what the change does. If you cannot summarize it clearly, keep reading until you can.

## Review Protocol: Seven-Agent Swarm

A Sworm review has **seven distinct scopes**. Each scope gets its own agent. Collapsing
scopes together forces one agent to context-switch between unrelated concerns and
causes later scopes to be skimmed. The swarm exists to keep each lens sharp.

For any non-trivial change, dispatch all seven agents in a **single tool-call
message** using the Agent tool. For a single-file or genuinely trivial change, you
may run the seven scopes inline in one pass — but still produce the same table.

Each agent inherits the Hard Gate reads and the Output Contract for Findings. Each
agent is capped at **5 rows by default**; go higher only when the scope genuinely has
more independent issues, never to pad. A narrow scope with few hits is a healthy
agent, not a weak one.

### Agent 1 — Architecture Fit

*Does the change respect the existing frontend/backend split?*

- Does frontend code stay in `src/` and privileged/native logic stay in Rust under
  `src-tauri/src/commands/` or `src-tauri/src/services/`?
- Does frontend data flow go through typed wrappers in `src/lib` instead of direct
  `invoke()` calls?
- Does the change solve a desktop problem, or does it treat Sworm like a browser-only
  web app?

Must read: `src/lib/api/backend.ts`, `src-tauri/src/commands/`, `src-tauri/src/services/`.

### Agent 2 — Primitive & Pattern Reuse

*Does the change reuse existing UI primitives, stores, and helpers?*

- Does it import project wrappers from `src/lib/components/ui/*` instead of using
  `bits-ui` directly in feature code?
- Does it create a new dialog, button, input, menu, or surface that should extend an
  existing primitive instead?
- Does it duplicate an existing flow, store, helper, or component instead of reusing it?

Must read: `src/lib/components/ui/dialog/index.ts`, `src/lib/components/ui/button/`,
`src/lib/components/ui/input/`, `src/lib/stores/confirmService.svelte.ts`,
`src/lib/components/ConfirmHost.svelte`.

### Agent 3 — Svelte 5 Idioms

*Is the Svelte 5 code actually idiomatic Svelte 5?*

- `$props()` instead of `export let`.
- `$state()` for mutable local state; no derived values stored in `$state`.
- `$derived()` / `$derived.by()` for computed state.
- `$effect()` only for real side effects — not to mirror state, derive state, or
  imitate React `useEffect`.
- `{#snippet}` / `{@render}` preferred over slots; `{@attach}` preferred over legacy
  `use:` actions.
- Shared state lives in `.svelte.ts` rune modules before reaching for context or
  classic stores.

Flag React cargo culting aggressively. Treating `$effect()` like `useEffect()` is a
real issue, not a suggestion.

Must read: the changed `.svelte` / `.svelte.ts` files and the nearest sibling
components for idiom comparison.

### Agent 4 — Tauri / Native Boundary

*Does the change respect Sworm's native path?*

- Prefer Tauri plugins or Rust for clipboard, filesystem, process, OS integration,
  and privileged work.
- Do not reach for browser APIs first when a native path exists.
- Do not introduce web-server assumptions into a desktop SPA.
- Do not add `tauri-plugin-shell`.

Must read: `src-tauri/Cargo.toml`, `src/lib/utils/clipboard.ts`, `src/lib/api/backend.ts`.

### Agent 5 — Architectural Coherence

*Does the change preserve or fragment architectural coherence?*

Distinct from Agent 1: that agent checks whether the existing split was followed.
This agent checks whether the change leaves the codebase *more coherent or more
fragmented* afterwards.

- Are responsibilities separated cleanly between component UI, stores / rune modules,
  typed frontend bridge, Tauri commands, and Rust services?
- Does it introduce a new abstraction only when an existing one is truly incompatible?
- Does it create two near-identical concepts that should be one extensible primitive?
- Does it scatter related pieces across too many files or create feature code with
  no coherent folder/module boundary?

Unnecessary new abstractions that increase duplication or fragment the design are
real issues.

Must read: `AGENTS.md`, the changed files in the context of their module structure,
and the nearest existing module that handles a similar concern.

### Agent 6 — Design System

*Does the UI match Sworm's established design system?*

- Tailwind utilities and existing design tokens from `src/app.css`.
- No raw colors, random typography changes, or ad hoc modal/button/input styling.
- Surface hierarchy (`ground` → `surface` → `raised` → `overlay`) respected.
- Focus/hover state consistency, icon sizing, radii, and motion match neighbors.
- Behaves and looks like nearby Sworm UI instead of inventing a new visual language.

Must read: `src/app.css`, neighboring components in the same feature area, existing
wrappers in `src/lib/components/ui/`.

### Agent 7 — Comment Style

*Do comments follow project conventions?*

- Sparse, useful, and non-obvious.
- Follow `.claude/comment-style/SKILL.md`.
- Flag misleading, stale, noisy, or AI-slop comments.
- Comments that explain obvious code are a real quality issue, not a nit.

Must read: `.claude/comment-style/SKILL.md` and the comment sites in the diff.

### Dispatch Template

Use one message with seven Agent tool calls in parallel. Each agent's prompt must
include:

1. The exact diff or file list under review.
2. Its agent number and scope question.
3. The "must read" list for that scope.
4. The Output Contract for Findings schema.
5. The 5-row default cap and the "go higher only for genuine independent issues" rule.

### Synthesis

Once all seven agents return, produce a single findings table:

- Merge all rows.
- Dedupe: if two agents flagged the same `path:line`, keep the higher-severity row
  and append the other agent's concern to the `Issue` cell (separated by `;`).
- Sort by severity (Blocking → Required → Suggestion), then by `Location`.
- Renumber the `#` column from 1.
- If total rows > 20, keep all Blocking and Required in the table and move
  Suggestions into a short bulleted appendix titled `### Suggestions` below the
  table. This preserves the high-severity signal when the diff is large.

If every agent returns zero rows, write `No findings.` once — not seven times.

## Sworm-Specific Red Flags

Flag these quickly when present:

- raw `bits-ui` imports in feature code instead of project wrappers
- direct `invoke()` use outside the typed backend bridge
- new confirm/prompt/dialog variants that overlap existing confirm/dialog primitives
- `$effect()` used for derived state, prop syncing, or general control flow
- Svelte 4 syntax in new Svelte 5 code
- browser-first clipboard or OS integration where Tauri/Rust already has support
- raw colors or one-off surfaces that ignore `src/app.css`
- duplicated components/helpers with only cosmetic differences
- creating a new component when a small extension of an existing primitive would be cleaner
- comments that ignore `.claude/comment-style/SKILL.md`

## Findings Bar

Use these severity levels:

### Blocking

- logic errors
- race conditions
- lifecycle breakage
- security/privacy issues
- broken desktop/native boundaries
- changes that meaningfully regress architecture or UI consistency

### Required Changes

- misuse of Svelte 5 runes
- primitive reuse failures
- duplication that should obviously be shared
- poor separation of concerns
- design-system drift
- misleading or low-quality comments

### Suggestions

- real but lower-risk improvements after Blocking and Required items are covered

Do not downgrade a real issue just because it is small. If it plausibly causes future churn, confusion, inconsistency, or bugs, it is an issue.

## Review Rules

- Use file and line references whenever possible.
- Explain the failure mode, not just the rule.
- When relevant, point to the existing Sworm pattern that should have been reused.
- If context is missing, say what you could not verify.
- If no findings are present, say so explicitly.
- For review-only requests, do not implement fixes unless the user asks.

## Non-Negotiable Output Contract for Findings

Findings are delivered as a **single markdown table**. Prose findings, bulleted findings,
and per-file sub-headings are disallowed. If a finding will not fit the schema below,
the finding is not ready — keep investigating until it does.

### Required Schema

| # | Severity | Location | Issue | Fix |
|---|----------|----------|-------|-----|
| 1 | Blocking | `src/lib/components/Foo.svelte:42` | `$effect` mirrors `selectedId` into a `$state` — prop changes now race with derived recomputes. | Replace with `$derived(() => props.selectedId)`; delete the mirror state. |
| 2 | Required | `src-tauri/src/commands/session.rs:88-96` | `invoke` result discards `Result::Err`, silently swallowing backend failures. | Return `Result<SessionId, String>`; propagate through the typed wrapper in `src/lib/api/backend.ts`. |

- One row per finding. Never split a finding across rows.
- `Severity` ∈ {Blocking, Required, Suggestion}. Rows are sorted Blocking → Required → Suggestion, then by `Location`.
- `Location` is always a code-formatted `path:line` or `path:start-end`. Multiple locations for one finding go comma-separated in one cell.
- `Issue` is one or two sentences naming the **failure mode**, not the rule name. "Violates Svelte 5 idioms" is not an issue; "`$effect` mirrors state that should be `$derived`, so re-renders lag one tick" is.
- `Fix` is imperative and specific. "Refactor this" is not a fix. "Extract `useTrackedAsyncLoad(key, load)` and consume it from both call sites" is.
- Zero findings → write `No findings.` in place of the table. Do not emit an empty table.

If you catch yourself composing a paragraph of findings, stop and convert to rows. The
schema exists because prose findings are unscannable and have been rejected repeatedly
in this project.

## Final Report Structure

The review response uses this layout, in order:

```md
## Summary
2-3 sentence summary of what the change does. Written before findings are composed,
per the Hard Gate.

## Findings
<the synthesized table, or `No findings.`>

## Open Questions / Assumptions
- What you could not verify.
- Context you had to assume.

## Verdict
Approve | Request Changes | Needs Discussion
```

The Summary goes first because it proves the Hard Gate was honored. The Findings
table follows the Output Contract for Findings — do not restate the rules here.