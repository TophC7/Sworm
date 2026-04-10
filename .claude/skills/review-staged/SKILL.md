---
name: review-staged
description: Review staged git changes against ADE conventions. Invoke with /review-staged before committing. Focus on correctness, scope control, and alignment with the current ADE specs.
---

# Review Staged Changes

Audit all staged git changes against project conventions, fix what you can, and report the rest.

**Execute this skill directly.** Do not dispatch sub-agents (no `Task` tool, no `Explore`, no `adversarial-code-reviewer`). Read the files, review them, fix them, run checks -- all in this session.

## Workflow

### 1. Identify staged files

Run `git diff --cached --name-only` to get the list of staged files. If nothing is staged, tell the user and stop.

Filter to relevant source/config files such as:

- `*.ts`
- `*.svelte`
- `*.svelte.ts`
- `*.js`
- `*.css`
- `*.rs`
- `*.nix`
- `*.toml`
- `*.json`
- `*.md`

Ignore generated and vendor output such as `node_modules/`, `build/`, `dist/`, and Tauri build artifacts.

### 2. Read project guidance (HARD GATE -- do not skip)

Load these references before reviewing. **Do not propose or apply any fixes until this step is complete.**

- **CLAUDE.md** (project root)
- **PHASE_0_1_TECH_SPEC.md**
- The staged file contents themselves (read every staged file completely)

After reading, write a brief summary of what the staged changes do (2-3 sentences). This confirms you understand the code before touching it. If you cannot summarize the changes, re-read until you can.

### 3. Review each file for issues

Check every staged file against these categories:

#### A. Comment style (FIX DIRECTLY)

Fix obviously bad or misleading comments directly. Keep comments sparse and functional.

#### B. TypeScript types (FIX WITHOUT REFACTORING)

- Loose types (`any`, untyped index signatures) that have a clear narrower alternative
- Missing type annotations on exported functions/parameters
- Unsafe casts that could use proper typed alternatives

Use judgment -- if narrowing a type requires refactoring the call sites, note it instead of fixing it.

#### C. Svelte/SvelteKit compliance (FIX DIRECTLY)

Check against **CLAUDE.md**, the current ADE specs, and the **svelte** MCP server to verify framework-specific correctness. Look for:

- Template issues (missing keys, wrong directive usage, deprecated APIs)
- Invalid component structure or misuse of runes
- Frontend code doing work that belongs in Rust/backend services

The MCP servers are more current than any hardcoded list -- use them.

#### D. Architecture (NOTE ONLY -- requires human review before any fix)

Look for structural problems that cross module boundaries or affect the app's design:

- Wrong boundary crossings (server/client imports, env var misuse)
- Duplicated definitions that should be shared
- Incomplete implementations or abstractions that drift from the ADE specs

These are presented in the results table for the user to decide on. Never fix these autonomously -- they often involve design decisions that need context beyond the diff.

#### E. Code quality & maintainability (NOTE ONLY -- requires human review before any fix)

Audit for dead code, separation of concerns, and unnecessary duplication:

- **Dead code** – if staged deletions leave orphaned imports, unused variables, or unreachable code in other files
- **Separation of concerns** – Rust owns privileged operations; frontend owns presentation/state wiring
- **Unnecessary creation** – new methods, components, or classes that duplicate existing functionality or violate YAGNI/SOLID/DRY/KISS principles
- **Coherence** – ensure new additions follow existing patterns and don't introduce inconsistent abstractions

These are presented in the results table for the user to decide on. Never fix these autonomously -- they involve architectural judgment and may affect other parts of the codebase.

### 4. Apply fixes

For categories A, B, and C: edit the files directly. Keep changes minimal and focused -- don't refactor, don't change behavior.

**Scope guard:** Only touch lines that are part of the staged diff. Do not "improve" nearby code that wasn't changed. Do not revert existing styling choices. If a line wasn't staged, it doesn't exist for this review.

### 5. Run checks

Run the narrowest relevant checks that actually exist in the repo. Prefer:

- `bun run check` for Svelte/TypeScript frontend checks
- `bun eslint src/` if ESLint is configured
- targeted formatter commands for touched files

If a referenced command does not exist yet, say so instead of inventing one.

If checks fail:

- Fix the errors
- Re-run
- Repeat until both pass

Then run `bun prettier --check` on the changed files. If formatting is off, run `bun prettier --write` on those files.

### 6. Verify fixes

After applying fixes and passing checks, **re-read every file you edited**. For each edit, confirm:

- The change does what you intended
- You didn't introduce new issues (broken imports, wrong indentation, mismatched brackets)
- You didn't touch lines outside the staged diff

If anything looks wrong, fix it now before presenting results.

### 7. Present results

Output a summary with three sections:

**Fixes applied** -- table with columns: File, What Changed, Why

**Issues found (not fixed)** -- table with columns: File, Issue, Problem, Potential Fix
These are category D items that need human decision.

**Check results** -- confirm what you ran and whether it passed.

**Commit message** -- draft a conventional commit message covering all changes. End with:

```
Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
```

## Rules

- Stay scoped to staged files ONLY. Don't review or fix unstaged code.
- Don't refactor. Don't change behavior. Don't add features.
- If a fix would change component APIs or behavior, note it instead of fixing it.
- Use the `svelte` MCP server to verify framework-specific issues when unsure.
- If no issues are found, say so clearly. Don't invent problems.
