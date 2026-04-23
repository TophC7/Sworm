---
name: sworm-design
description: Sworm's authoritative design system. Tokens, surfaces, type scale, shadows, motion, iconography, voice, and component recipes. Use this skill whenever you touch UI in Sworm — building new components, refactoring styling, updating primitives, reviewing design choices, or designing a new surface (dialog, panel, row, menu). Trigger on any edit to src/app.css, src/lib/components/ui/**, or Svelte files under src/lib. Also use when questions arise about Sworm's palette, typography, surface hierarchy, focus states, shadows, radii, or iconography. Supersedes extensions/handoff/ and extensions/Sworm Design System/.
---

# Sworm Design System

Warm base16 terminal palette. Peach accent. Dark-only. Dense by default. Flat chrome, rounded popovers, one accent color used sparingly. Type is small, tight, unhurried. The interface should disappear behind the work.

This file is the single source of truth. The live tokens live in `src/app.css`. When the two disagree, this file wins and `src/app.css` gets updated to match.

---

## 1 · How the system is consumed

Three places interact with tokens:

| Surface                                            | What it uses                             | Rule                                                                                     |
| -------------------------------------------------- | ---------------------------------------- | ---------------------------------------------------------------------------------------- |
| `src/app.css` `@theme` block                       | The canonical token set                  | Edit here first; Tailwind v4 emits utilities from it.                                    |
| Svelte components under `src/lib/components/ui/**` | Tailwind utilities emitted from `@theme` | Consume via utilities (`bg-surface`, `text-base`, `shadow-popover`). Never hardcode hex. |
| Feature components under `src/lib/components/**`   | Primitives from `ui/*`                   | Never reach for `bits-ui` directly. Never restyle what a primitive already does.         |

If you need a token that does not exist, add it to `@theme`, update this doc in the same commit.

---

## 2 · Color tokens

### 2.1 Base16 reference

Sworm's palette is a warm base16 scheme. These are the raw ANSI slots. You rarely reference them directly; the named roles below are the usable API.

| Slot | Hex       | Role                |
| ---- | --------- | ------------------- |
| 00   | `#131313` | ground / black      |
| 01   | `#ff7672` | danger / red        |
| 02   | `#98ff7f` | success / green     |
| 03   | `#ffe572` | warning / yellow    |
| 04   | `#f29d84` | accent-dim / orange |
| 05   | `#763724` | warm brown          |
| 06   | `#ffb59f` | accent / peach      |
| 07   | `#fff3ef` | bright              |
| 08   | `#a59c99` | muted gray          |
| 09   | `#ffa29f` | danger-bright       |
| 10   | `#b7ffa5` | success-bright      |
| 11   | `#ffeea5` | warning-bright      |
| 12   | `#ffc0ad` | accent-bright       |
| 13   | `#ffcbbb` | pink                |
| 14   | `#ffddd3` | peach               |
| 15   | `#fffaf8` | max                 |

### 2.2 Surface hierarchy

Four steps, darkest to lightest. Every element sits on exactly one.

| Token     | Hex       | Where it lives                                                                                           |
| --------- | --------- | -------------------------------------------------------------------------------------------------------- |
| `ground`  | `#131313` | App background, terminal pane, editor canvas, active project tab fuses with this.                        |
| `surface` | `#201c1a` | Title bar, status bar, sidebars, panel headers, inputs.                                                  |
| `raised`  | `#2f2926` | Default buttons, active sidebar tabs, command palette body, cards inside a panel, tooltip/dropdown body. |
| `overlay` | `#3c3532` | Menus, context menus, tooltips when they need extra lift above a `raised` surface.                       |

**Stepping rule.** Step up or down when you nest. Never same-level nesting (`raised` inside `raised`). If the container is `raised`, children step to `surface` (down) or `overlay` (up). Feature cards inside a `bg-raised` dialog go to `bg-overlay`; an input inside an `bg-overlay` card goes to `bg-surface`.

### 2.3 Borders

| Token         | Hex       | Use                                                              |
| ------------- | --------- | ---------------------------------------------------------------- |
| `edge`        | `#3a322e` | Default — dividers, panel borders, input borders, card outlines. |
| `edge-strong` | `#4a403a` | Emphasized separators under pressure. Rare.                      |

### 2.4 Foreground hierarchy

Strict. Darker values carry less visual weight. Use the _lowest_ level that works — the whole interface should feel calm.

| Token    | Hex       | Use                                                                               |
| -------- | --------- | --------------------------------------------------------------------------------- |
| `max`    | `#fffaf8` | Top-of-hierarchy labels: on-accent text (black-on-peach buttons), hero wordmarks. |
| `bright` | `#fff3ef` | Dialog titles, active states, card titles, hover-emphasized text.                 |
| `fg`     | `#e2e2e2` | Body text, default content, form labels.                                          |
| `muted`  | `#a59c99` | Secondary text, captions, default icon color, inactive tab labels.                |
| `subtle` | `#6e6864` | Disabled text, timestamps, tertiary detail, placeholder.                          |

### 2.5 Accent

One warm peach. The brand color. Used for activity, focus, the active-view indicator in the activity bar, primary CTAs, commit hashes, the agent "working" pulse.

| Token           | Hex                      | Use                                                                                 |
| --------------- | ------------------------ | ----------------------------------------------------------------------------------- |
| `accent`        | `#ffb59f`                | Foreground accent. Text, icons, focused input border, tab underline.                |
| `accent-dim`    | `#f29d84`                | Filled accent surface — primary CTA background.                                     |
| `accent-bright` | `#ffc0ad`                | Hover of a filled accent surface. Primary button hover goes _brighter_, not darker. |
| `accent-bg`     | `rgba(255,181,159,0.13)` | Tinted fill for selected rows, active-accent chips.                                 |

**Rule.** One accent, used sparingly. If you catch yourself painting decoration with it, stop.

### 2.6 Semantic

| Purpose     | `fg`      | `bright`         | `bg`         | `border`        |
| ----------- | --------- | ---------------- | ------------ | --------------- |
| **Danger**  | `danger`  | `danger-bright`  | `danger-bg`  | `danger-border` |
| **Success** | `success` | `success-bright` | `success-bg` | `success/40`    |
| **Warning** | `warning` | `warning-bright` | `warning-bg` | `warning/40`    |

**Alert recipe:**

```html
<div class="border border-danger-border bg-danger-bg text-danger-bright">…</div>
```

Session state lives on **status dots**, not on badges or alert banners.

### 2.7 Extended decorative

`selection` (text-selection bg), `warm` (deep brown), `pink`, `peach` (peach-light). Use only for decorative highlights. Never for semantic state.

---

## 3 · Type

### 3.1 Families

**Sans:** Lexend variable (100–900). Everything that isn't code, a path, or a key cap. Trusts Lexend's wide defaults.

**Mono:** Monocraft Nerd Font. Always. Falls back to the OS default `monospace` only — no SF Mono chain, no JetBrains, no Cascadia.

Mono is for _data_: session IDs, file paths, kbd keys, timestamps, branch names, kicker labels, terminal output.

### 3.2 Scale

All sizes ship as Tailwind utilities (`text-2xs` … `text-5xl`). Use the named scale; no new `text-[…]` literals in new code.

| Utility     | Size             | Line height | Weight default | Use                                               |
| ----------- | ---------------- | ----------- | -------------- | ------------------------------------------------- |
| `text-2xs`  | 10.4px · 0.65rem | 1.2         | 500            | Status bar micro-labels, badges, kbd.             |
| `text-xs`   | 10.9px · 0.68rem | 1.25        | 500            | Section labels (UPPERCASE), button-xs, subtitles. |
| `text-sm`   | 11.5px · 0.72rem | 1.3         | 400            | Small buttons, dense list rows, tooltips, tabs.   |
| `text-base` | 13.1px · 0.82rem | 1.4         | 400            | **Default UI body.** Buttons, inputs, labels.     |
| `text-md`   | 13.6px · 0.85rem | 1.4         | 600            | Card titles, provider labels, emphasized rows.    |
| `text-lg`   | 14.4px · 0.9rem  | 1.4         | 600            | Section headers inside dialogs.                   |
| `text-xl`   | 16px · 1rem      | 1.35        | 700            | Dialog titles, view headers.                      |
| `text-2xl`  | 17.6px · 1.1rem  | 1.3         | 700            | Card titles in hero contexts.                     |
| `text-3xl`  | 20px · 1.25rem   | 1.2         | 700            | Empty-state headings, section hero.               |
| `text-4xl`  | 24px · 1.5rem    | 1.15        | 500            | Settings H2, page titles.                         |
| `text-5xl`  | 32px · 2rem      | 1.1         | 500            | Onboarding hero, wordmark.                        |

**Weights move with the scale.** Chrome sizes (`xxs`/`xs`) sit at 500, body sizes (`sm`/`base`) at 400, emphasis (`md`/`lg`) at 600, headings (`xl`/`2xl`) at 700, hero (`4xl`/`5xl`) at 500 so the warm background does the emotional lifting.

### 3.3 Tracking

- Default tracking for Lexend.
- `tracking-wide` + `uppercase` on section labels (`START`, `RECENT`).
- `tracking-tight` (letter-spacing: -0.01em) on hero wordmarks only.

---

## 4 · Iconography

### 4.1 Lucide for UI chrome

- All UI icons come from `@lucide/svelte`, re-exported via `src/lib/icons/lucideExports.ts`.
- Default stroke width 2.
- Sizes by context:
  - **9–11px** — status bar, badges.
  - **12px** — inline with `text-sm` chrome (kbd hints, command palette icons).
  - **14–16px** — buttons, menu items, panel header actions.
  - **18px** — titlebar logo.
  - **28–30px** — empty-state hero icons.
- Color inherited from `color:`. Match the surrounding text role (`text-muted` secondary, `text-bright` hover, `text-accent` active, `text-danger`/`text-success`/`text-warning` for semantics).
- When adding a new icon, re-export it from `lucideExports.ts` with the `*Icon` suffix convention (`MenuIcon`, `SearchIcon`, `SettingsIcon`). Exceptions kept for short iconic names (`X`, `Plus`, `Check`, `Worm`).

### 4.2 Brand SVGs

Bundled under `static/svg/` (and mirrored into `extensions/Sworm Design System/assets/` for external mocks):

- `worm.svg`, `sworm.svg` — app logo; worm is 18px titlebar, 30px empty-state.
- Provider marks: `claudecode.svg`, `codex.svg`, `copilot.svg` (PNG fallback), `gemini.svg`, `opencode.svg`, `fresh.svg`, plus `crush.png`.
- `nixos.svg`, `terminal.svg`.

**Provider presence rule.** Provider logos render full-color when the provider is detected, `grayscale + 50% opacity` when not. This is how Sworm communicates "connected vs not available" — no extra dot, no extra label.

### 4.3 File-type icons

`static/icons/bearded/*.svg` (Bearded Icons, ~400 files) map to extensions via `src/lib/icons/fileIconMap.ts`. Used exclusively by the file tree.

### 4.4 Emoji ban

Never. No unicode-character-as-icon either. If you need a glyph, use Lucide or a bundled brand SVG.

---

## 5 · Radii

Sworm is flat by default. Rounding exists only where a surface visibly _floats_ or a chip explicitly wants to read as a pill.

| Utility        | Size   | Use                                                                                                             |
| -------------- | ------ | --------------------------------------------------------------------------------------------------------------- |
| `rounded-none` | 0      | Title bar chrome, tabs, status bar, activity bar icons, window controls, panel headers. The default for chrome. |
| `rounded-sm`   | 2px    | Tiny chips, kbd shadows at low resolution.                                                                      |
| `rounded`      | 4px    | Inputs, small buttons, the command pill. The lightest rounding that reads as "this element is a control."       |
| `rounded-md`   | 6px    | Kbd chips, code blocks, secondary buttons.                                                                      |
| `rounded-lg`   | 8px    | Default buttons, primary CTAs, dialog content, dropdown content, context menu content. Floating surfaces.       |
| `rounded-xl`   | 12px   | Command palette, provider cards, markdown-rendered `kbd`. The largest used anywhere in chrome.                  |
| `rounded-full` | 9999px | Status dots and avatars only. Never on buttons or pills.                                                        |

**Rule.** Chrome (title bar, sidebars, tabs, status bar) is square. Only things that _detach from_ the chrome — buttons, dialogs, menus, tooltips, the command palette, the command pill — carry rounding. Status dots are circles because they are dots.

---

## 6 · Shadows

Elevation shadow is reserved for **floating surfaces only**. Cards, buttons, inputs, sidebars, panel headers — no shadow. They use surface steps + borders to establish hierarchy.

| Token               | Value                              | Use                                                                        |
| ------------------- | ---------------------------------- | -------------------------------------------------------------------------- |
| `shadow-popover`    | `0 16px 48px rgba(0,0,0,0.5)`      | Command palette, dropdowns, context menus, tooltips, toasts. Nothing else. |
| `shadow-kbd-inset`  | `inset 0 -1px 0 var(--color-edge)` | Kbd chips — the key-cap bottom-edge cue.                                   |
| `shadow-focus-ring` | `0 0 0 2px rgba(255,181,159,0.35)` | Focus outline for interactive non-input elements. See §8.                  |

**Never use Tailwind's default `shadow-sm` / `shadow-md`**. Their values don't match the warm background. If you feel a card needs elevation, you probably need to step to the next surface token instead.

---

## 7 · Motion

### 7.1 Easing

| Utility          | Value                           | Use                                                           |
| ---------------- | ------------------------------- | ------------------------------------------------------------- |
| `ease-out-expo`  | `cubic-bezier(0.16, 1, 0.3, 1)` | Dramatic entrances — dialog appear, command palette drop.     |
| `ease-out-quart` | `cubic-bezier(0.25, 1, 0.5, 1)` | Default for everything else.                                  |
| `ease-default`   | `cubic-bezier(0.2, 0, 0, 1)`    | Pane-level transitions where `ease-out-quart` feels too soft. |

### 7.2 Duration

| Utility            | Value | Use                                                            |
| ------------------ | ----- | -------------------------------------------------------------- |
| `duration-instant` | 80ms  | Color shifts on color-only hover that shouldn't feel animated. |
| `duration-fast`    | 150ms | **Default** for color/border/opacity transitions.              |
| `duration-med`     | 240ms | Dropdown open, tooltip appear, toast in.                       |
| `duration-slow`    | 400ms | Dialog open, pane drag settle.                                 |

### 7.3 Named animations

| Class          | What it does                             | Where                                                    |
| -------------- | ---------------------------------------- | -------------------------------------------------------- |
| `pulse-accent` | 1400ms opacity pulse on the accent color | The active-session working indicator (status dot + tab). |
| `beam-sweep`   | 3s horizontal gradient sweep             | Active project tab underline.                            |

Neither is generic. If you want to animate something else, use color/opacity transitions with the duration + easing tokens above.

### 7.4 Interaction rules

- **No scale on press.** Sworm is a dev tool; shrinking on click feels wrong.
- **Color shifts toward `bright`**, never brightness filters.
- **Resize handles are instant.** No animation on drag.
- **Entrance fade** via `BlurFade` (motion-sv) stays limited to stage-level entrances: empty state, new-session picker. Don't blanket every render.

---

## 8 · Focus states

Sworm uses two mechanisms depending on element type. Pick the right one per primitive.

### 8.1 Border-swap (inputs)

Inputs, textareas, the command input — change border color on focus.

```html
<input class="border border-edge bg-surface outline-none focus:border-accent" />
```

No outline, no ring. The border change is the whole focus indicator.

### 8.2 Focus ring (non-input interactive elements)

Buttons, icon buttons, tabs, menu triggers, list rows — show a ring on `:focus-visible` only so it stays invisible to mouse users.

```html
<button class="focus-visible:shadow-focus-ring focus-visible:outline-none">…</button>
```

**Never use Tailwind's `ring-*` utilities.** They layer another shadow on top of whatever the element already has; we use the token `shadow-focus-ring` directly so it's clearly a focus indicator, not decoration.

If an element already has a `shadow-popover` (dropdowns, tooltips that are themselves focusable), you may combine with `shadow-[var(--shadow-focus-ring),var(--shadow-popover)]` — the focus ring wins on top.

---

## 9 · Spacing & density

Sworm is dense but not cramped. Stay on the Tailwind 4px scale. Common rhythms:

- **Inline gap** — `gap-1.5` (6px) tight, `gap-2.5` (10px) default, `gap-4` (16px) generous.
- **Row height** — `h-7`/`h-8` (28–32px) for list rows, `h-6` (24px) for compact sidebars.
- **Card padding** — `p-3` default, `p-4`/`p-5` for dialog content.
- **Panel headers** — `px-3 py-2` (12px horiz × 8px vert).
- **List item padding** — `px-3 py-1.5`.

Fixed chrome dimensions (these are load-bearing):

- Title bar: **36px** min height.
- Activity bar: **36px** wide (matches the titlebar logo column for vertical alignment).
- Status bar: **24px** tall.
- Kbd chip: **24px** tall (`h-6`).
- Status dot: **8px** circle.
- Command pill: **26px** tall.

If you catch yourself writing `p-[11px]` or `h-[33px]`, step back to one of the above.

---

## 10 · Voice & tone

Terse, terminal-native, developer-to-developer. No marketing copy. The reader has shipped software before.

### Casing

- **Title Case** — menu labels, buttons, tab titles (`Open Repository`, `New Session`, `Discard All`).
- **UPPERCASE + letter-spacing** — sidebar section dividers (`START`, `RECENT`), via the `.section-label` utility.
- **lowercase.with-dots** / **kebab-case** — filenames, branches, commands (`main`, `src-tauri`, `app:dev`).
- **Sentence case** — longer prose inside dialogs and empty-state bodies.

### Pronouns

- **"You"** is used sparingly and only when the user performed the action: _"You have 3 unsaved files."_
- Never _"we"_ or _"our"_. Sworm does not personify itself.

### Vocabulary

- **Session** — a running coding-agent process tied to a project.
- **Project** — a local git repository the user has added.
- **Provider** — the backing CLI.
- **Pane / Tab** — visual containers inside a project view.
- **Stage / Unstage / Discard / Stash** — git verbs, never translated.
- **Live / Idle / Exited / Failed** — session statuses.

### Examples

- Empty state: _"Sworm"_ / _"Agentic Development Environment"_.
- Destructive confirm: _"This will permanently discard all unstaged changes and remove untracked files. This cannot be undone."_
- Session exit: _"Session exited. Showing restored terminal history."_

### Banned

- Emoji. Emoticons. Exclamation marks in chrome.
- _"Let's get started!"_, _"Sure thing!"_, _"Oops!"_, _"Whoops"_, _"Awesome!"_.
- Generic "Are you sure?" confirms. State what will happen, factually.

---

## 11 · Component recipes

These are the authoritative sizes + variants for each primitive. If a primitive in `src/lib/components/ui/*` disagrees with what's here, the primitive is wrong.

### 11.1 Component shape

Every primitive under `ui/*` follows this exact shape:

```svelte
<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const fooVariants = tv({
    base: '…',
    variants: { variant: { … }, size: { … } },
    defaultVariants: { variant: '…', size: '…' }
  })

  export type FooVariant = VariantProps<typeof fooVariants>['variant']
  export type FooSize = VariantProps<typeof fooVariants>['size']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    variant = '…',
    class: className,
    children,
    ...rest
  }: HTMLAttributes<HTMLElement> & {
    variant?: FooVariant
    class?: string
    children?: Snippet
  } = $props()
</script>

<element data-slot="foo" class={cn(fooVariants({ variant }), className)} {...rest}>
  {#if children}{@render children()}{/if}
</element>
```

Rules:

- `<script lang="ts" module>` for variants + types.
- `data-slot="foo"` on root.
- `cn(fooVariants({…}), className)` — user `className` always last so callers can override.
- `{...rest}` spread last.
- `{#if children}{@render children()}{/if}` — never unconditional.
- Filename is kebab-case (`session-row.svelte`).
- Folder has `index.ts` re-exporting default + variant types.

### 11.2 Button

`variant`: `default | ghost | outline | destructive | accent`
`size`: `default | sm | xs | icon | icon-sm`

| Variant     | Classes                                                                           |
| ----------- | --------------------------------------------------------------------------------- |
| default     | `bg-raised border border-edge text-fg hover:border-accent hover:text-bright`      |
| ghost       | `bg-transparent border-none text-muted hover:bg-surface hover:text-bright`        |
| outline     | `bg-transparent border border-edge text-fg hover:border-accent hover:text-bright` |
| destructive | `bg-danger-bg border border-danger-border text-danger hover:text-danger-bright`   |
| accent      | `bg-accent-dim border border-accent-dim text-ground hover:bg-accent`              |

| Size    | Classes                 |
| ------- | ----------------------- |
| default | `px-3.5 py-2 text-base` |
| sm      | `px-2.5 py-1 text-sm`   |
| xs      | `px-2 py-0.5 text-xs`   |
| icon    | `w-7 h-7 p-0`           |
| icon-sm | `w-5 h-5 p-0`           |

Base: `inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50`.

### 11.3 IconButton

Wraps `Button` with tooltip + `aria-label`. Always accepts `tooltip: string` and an optional `shortcut: string`. For titlebar/chrome flat icon buttons, use a `shape: 'flat'` variant (see roadmap — not yet landed).

### 11.4 Input

`bg-surface border border-edge rounded px-2.5 py-1.5 text-base text-fg outline-none placeholder:text-subtle focus:border-accent transition-colors`.

Textarea identical except `resize-none`.

Focus uses border-swap (see §8.1). No outline.

### 11.5 Kbd

`inline-flex items-center justify-center h-6 min-w-6 px-1.5 rounded-md border border-edge bg-surface font-mono text-sm font-medium text-fg shadow-kbd-inset select-none pointer-events-none`.

### 11.6 Badge

Pill, `rounded-full`. `inline-flex items-center px-2 py-0.5 text-xs uppercase tracking-wide font-medium`.

| Variant | Classes                      |
| ------- | ---------------------------- |
| default | `bg-edge text-muted`         |
| success | `bg-success/15 text-success` |
| warning | `bg-warning/15 text-warning` |
| danger  | `bg-danger/15 text-danger`   |
| accent  | `bg-accent-bg text-accent`   |

Badges carry **information**, not state. Session state lives on status dots.

### 11.7 Status dot

`inline-block h-2 w-2 rounded-full shrink-0`. Size `sm` = `h-1.5 w-1.5`, `lg` = `h-2.5 w-2.5`.

| Status    | Fill         | Animation      |
| --------- | ------------ | -------------- |
| `idle`    | `bg-muted`   | —              |
| `working` | `bg-accent`  | `pulse-accent` |
| `waiting` | `bg-warning` | —              |
| `success` | `bg-success` | —              |
| `danger`  | `bg-danger`  | —              |

Active states (`working`, `danger`) may add a 3px halo via `shadow-[0_0_0_3px_rgba(…)]` at 15% opacity.

### 11.8 Tab

**Project variant** (lives in titlebar): `h-8 rounded-none`. Active = `bg-raised text-bright border border-edge border-b-0` + animated **bottom** accent line (2px, `bg-accent`, `beam-sweep` 3s). Inactive = `hover:bg-raised hover:text-bright`.

The beam is always `accent`. Session state is carried by the status dot inside the tab, not by the beam color.

**Pane variant** (sessions/files/diffs/commits): `self-stretch text-sm`. Active = `bg-surface text-fg`. No beam; the bg change is the indicator. Temporary (preview) tabs render italic until promoted via double-click.

Close affordance: hidden until hover, `text-muted hover:bg-edge hover:text-bright`.

### 11.9 Dialog

`DialogContent` = `bg-raised border border-edge rounded-2xl shadow-popover p-5`. Inside it, cards step to `bg-overlay`; inputs step to `bg-surface`. Dialog title uses `text-xl font-semibold text-bright`.

Overlay: `fixed inset-0 bg-ground/70 backdrop-blur-sm`. The one place blur is used.

### 11.10 Dropdown / context menu

`DropdownMenuContent` = `bg-raised border border-edge rounded-lg py-1 text-base shadow-popover min-w-[180px]`.

Items = `px-3 py-1.5 text-fg hover:bg-surface focus:bg-surface rounded-sm`. Disabled items → `text-subtle cursor-default`.

Separator = `mx-2 my-1 h-px bg-edge`.

### 11.11 MagicCard (exception)

`src/lib/components/ui/magic-card/magic-card.svelte` is the **one** decorative surface in the system. Cursor-following radial peach gradient overlay.

**Scope is tight:** currently used only on the New Session provider picker. Do not extend to other surfaces without updating this doc first. If a new use case appears, propose it.

### 11.12 Command palette

`bg-raised border border-edge rounded-xl shadow-popover` with `Dialog.Overlay` backdrop. See the live `CommandCenter.svelte` for the canonical layout.

### 11.13 Toast / notification

Use `shadow-popover` + surface `bg-overlay`. Enter with `toastIn` (fade + slight Y offset, `duration-med` with `ease-out-expo`).

### 11.14 Alert (inline, non-floating)

```html
<div class="border-{semantic}/40 bg-{semantic}-bg text-{semantic}-bright border">…</div>
```

Variants: `info` (use `accent`), `success`, `warning`, `danger`. Leading icon uses the strong color (`[&>svg]:text-{semantic}`). Alerts are inline — no shadow.

---

## 12 · Do / Don't

### Do

- Use semantic tokens (`bg-surface`, `text-muted`, `border-edge`) exclusively.
- Use the named type scale (`text-base`, `text-md`, `text-lg`).
- Use `tailwind-variants` for every primitive with >1 state.
- Pass `className` through `cn()` with user class **last**.
- Use mono for data (IDs, paths, timestamps, kbd chips).
- Use status dots for session/process state.
- Use surface _steps_ to imply hierarchy; skip shadows.
- Pulse accent only on things that are actively working.

### Don't

- Don't write raw hex codes in Svelte files.
- Don't use Tailwind palette colors (`text-orange-400`, `bg-slate-800`).
- Don't add new `text-[…]` literals. Named scale only.
- Don't nest same-level surfaces.
- Don't use `ring-*`. Use `shadow-focus-ring` or border-swap.
- Don't use `shadow-sm` / `shadow-md` / other Tailwind default shadows.
- Don't gradient-fill backgrounds. MagicCard is the only exception.
- Don't write `<slot>`. Children are `Snippet` props rendered with `{@render children()}`.
- Don't emoji. Don't unicode-icon. Don't exclamation-mark in chrome.
- Don't reach for `bits-ui` in feature code. Use `ui/*` wrappers.
- Don't copy-paste a 300-char class string. Extract a primitive.

---

## 13 · Review checklist

Before handing UI work back, run this against your diff.

1. **Svelte 5.** No `export let`, no `$:`, no `createEventDispatcher`, no `<slot>`. `$props() / $state / $derived / $effect` only. `$effect` only for real side effects, never to mirror state.
2. **TypeScript.** Every `<script>` is `<script lang="ts">`. Props explicitly typed. Variant types exported from module script. No `any`.
3. **Tokens.** Grep diff for `\#[0-9a-fA-F]{3,8}` → no results in `.svelte` files. No palette colors. No new `text-[…]` literals.
4. **Component shape.** `data-slot` on root. `cn(variants({…}), className)` with user class last. `{#if children}{@render children()}{/if}`. `{...rest}` last.
5. **Surfaces.** Every element on exactly one of `ground | surface | raised | overlay`. No same-level nesting.
6. **Accent.** Used only for activity, focus, primary CTA, working indicators. Pulse only on working things.
7. **Focus.** Input = border-accent swap. Everything else = `focus-visible:shadow-focus-ring`.
8. **Shadows.** Only on floating/popover surfaces.
9. **Motion.** Uses named easing + duration tokens. Not hand-rolled cubic-beziers.
10. **Voice.** No emoji, no fluff, Title Case on CTAs.
11. **Primitive reuse.** No raw `bits-ui` imports in feature code. No restyle of what a primitive already does.

Output a short block at the end of a UI task:

```
Design review:
✓ Svelte 5 runes, no export let / $: / dispatchers / slots
✓ TS typed, variant types exported
✓ Tokens only — grepped for hex, none
✓ Surfaces compose cleanly (surface → raised → overlay)
✓ Accent confined to <specific interactions>
✓ Focus uses shadow-focus-ring / border-accent as appropriate
✓ No forbidden shadows
✓ Named motion tokens used
✓ Voice matches house style
```