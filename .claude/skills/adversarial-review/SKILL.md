---
name: sworm-adversarial-review
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

## Sworm Review Lens

Check every change against these questions.

### 1. Does it fit Sworm's actual architecture?

- Does frontend code stay in `src/` and privileged/native logic stay in Rust under `src-tauri/src/commands` or `src-tauri/src/services`?
- Does frontend data flow go through typed wrappers in `src/lib` instead of direct `invoke()` calls?
- Does the change solve a desktop problem as if this were a browser-only web app?

Authoritative examples:

- `src/lib/api/backend.ts`
- `src-tauri/src/commands/`
- `src-tauri/src/services/`

### 2. Does it reuse existing primitives and patterns?

- Does it import project wrappers from `src/lib/components/ui/*` instead of using `bits-ui` directly in feature code?
- Does it create a new dialog, button, input, menu, or surface that should extend an existing primitive instead?
- Does it duplicate an existing flow, store, helper, or component instead of reusing it?

Authoritative examples:

- `src/lib/components/ui/dialog/index.ts`
- `src/lib/components/ui/button/`
- `src/lib/components/ui/input/`
- `src/lib/stores/confirmService.svelte.ts`
- `src/lib/components/ConfirmHost.svelte`

### 3. Is the Svelte 5 code actually idiomatic Svelte 5?

- Use `$props()` instead of `export let`
- Use `$state()` for mutable local state
- Use `$derived()` or `$derived.by()` for computed state
- Use `$effect()` only for real side effects, not to mirror state, derive state, or imitate React `useEffect`
- Keep shared state in `.svelte.ts` rune modules before reaching for broader patterns

Flag React cargo culting aggressively. A change that treats `$effect()` like `useEffect()` is a real issue.

### 4. Does it respect the Tauri/native boundary?

- Prefer Tauri plugins or Rust for clipboard, filesystem, process, OS integration, and privileged work
- Do not reach for browser APIs first when the app already has a native path
- Do not introduce web-server assumptions into a desktop SPA
- Do not add `tauri-plugin-shell`

Authoritative examples:

- `src-tauri/Cargo.toml`
- `src/lib/utils/clipboard.ts`
- `src/lib/api/backend.ts`

### 5. Does it preserve architectural coherence?

- Are responsibilities separated cleanly between component UI, stores/rune modules, typed frontend bridge, Tauri commands, and Rust services?
- Does it introduce a new abstraction only when the existing one is truly incompatible?
- Does it create two near-identical concepts that should be one extensible primitive?
- Does it scatter related pieces across too many files or create feature code with no coherent folder/module boundary?

Treat unnecessary new abstractions as a real issue when they increase duplication or fragment the design.

### 6. Does it match the established design system?

- Does it use Tailwind utilities and existing design tokens from `src/app.css`?
- Does it avoid raw colors, random typography changes, and ad hoc modal/button/input styling?
- Does it look and behave like nearby Sworm UI instead of inventing a new visual language?

Authoritative examples:

- `src/app.css`
- neighboring components in the same feature area
- existing wrappers in `src/lib/components/ui/`

### 7. Do the comments follow project conventions?

- Comments should be sparse, useful, and non-obvious
- Follow `.claude/comment-style/SKILL.md`
- Flag misleading, stale, noisy, or AI-slop comments
- If comments explain obvious code, that is a real quality issue

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

- Findings first. Do not lead with a summary.
- Use file and line references whenever possible.
- Explain the failure mode, not just the rule.
- When relevant, point to the existing Sworm pattern that should have been reused.
- If context is missing, say what you could not verify.
- If no findings are present, say so explicitly.
- For review-only requests, do not implement fixes unless the user asks.

## Response Format

Use this structure:

```md
## Findings
1. [Severity] file:line - issue
   Why it matters in Sworm.
   Preferred local pattern or file to compare against.

## Open Questions / Assumptions
- ...

## Summary
- Brief overall assessment
- Verdict: Approve | Request Changes | Needs Discussion
```
