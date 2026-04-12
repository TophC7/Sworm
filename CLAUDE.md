# Sworm

Sworm is a Linux-first desktop app for running coding-agent CLIs inside a local git repository.


## Stack

- Development environment: Nix-first
- Desktop runtime: Tauri v2
- Frontend: SvelteKit
- Backend/runtime: Rust
- Database: SQLite
- CSS framework: Tailwind CSS v4 (via `@tailwindcss/vite`)
- Package manager: `bun`


## Working Rules

- Nix is the source of truth for development environment and packaging.
- Prefer `nix develop` and `nix build` over ad hoc host setup.
- Use `bun`, not `pnpm` or `npm`.
- Use Bun through the Nix environment; manage Bun dependency reproducibility with `bun2nix`.
- Keep privileged operations in Rust, not in the frontend.
- Do not add `tauri-plugin-shell` for app runtime process spawning.
- Use the system `git` CLI rather than `git2` unless the repo explicitly changes direction.
- Keep the frontend as a SvelteKit SPA with SSR disabled.
- Match the existing module split:
  - Svelte routes/components/stores in `src/`
  - Tauri commands in `src-tauri/src/commands/`
  - business logic in `src-tauri/src/services/`
  - serialized models in `src-tauri/src/models/`
- Keep sessions project-scoped and be explicit when behavior is non-isolated.

## Validation

Use the narrowest relevant checks that exist today:

- `bun run check`
- `bun run build`
- `cargo check`
- `cargo test`
- `bun tauri build --debug`

Do not claim lint/format/test commands exist if they are not defined in the current repo.

## Svelte Rules

- Treat the frontend as a client-side SvelteKit SPA inside Tauri, not as a server-rendered web app.
- Use Svelte 5 patterns only. Do not introduce Svelte 4 syntax in new code.
- Use `$props()` instead of `export let`.
- Use `$state()` for mutable local state.
- Use `$derived()` or `$derived.by()` for computed state. Do not store derived values in `$state()`.
- Use `$effect()` only for real side effects such as DOM work, subscriptions, or browser/Tauri API integration. Do not use it to mirror or derive state.
- Prefer `{#snippet}` and `{@render}` over slots for component composition.
- Prefer `{@attach ...}` over legacy `use:` actions when attachments are needed.
- Keep state close to where it is used. Extract shared state into `.svelte.ts` rune modules before reaching for context.
- Avoid classic Svelte stores for new shared state unless an external library forces that shape.

## SvelteKit Rules

- Root SSR stays disabled. Do not reintroduce server-rendered assumptions.
- Do not add `+page.server.ts`, `+layout.server.ts`, `hooks.server.ts`, or remote-function-based app logic for the desktop runtime.
- Frontend data flow should go through typed Tauri invoke/channel wrappers in `src/lib`, then into component state or shared rune modules.
- Guard DOM-only or browser-only code with `onMount` or a narrowly scoped `$effect()`.
- Prefer simple route components and app-shell state over web-centric patterns such as auth redirects, SEO metadata work, or server-first data loading.
- Check existing ADE patterns before introducing a new component, state, or routing style.

## Styling Rules

- Use Tailwind utility classes for all styling. Do not add scoped `<style>` blocks unless Tailwind cannot express the rule (e.g. pseudo-elements, complex child selectors).
- When CSS is unavoidable, use `@apply` with design tokens from `src/app.css` instead of raw hex values.
- Design tokens are defined in `src/app.css` under `@theme`. Use semantic names (`bg-surface`, `text-muted`, `border-edge`) not raw colors.
- The color palette is a warm base16 scheme. All palette colors are available as Tailwind utilities (e.g. `text-accent`, `bg-danger-bg`, `text-success`).
- Surface hierarchy: `ground` (darkest) → `surface` → `raised` → `overlay` (lightest).
- Use `group` + `group-hover:` for parent-hover-reveals (e.g. remove buttons).
- Use `transition-colors` for hover/focus color transitions.

## Important Notes

- Terminal/session lifecycle correctness matters more than adding surface features quickly.
- Multiple sessions may exist in one project, but they still share the same working tree unless and until worktrees land.