# Sworm Primitive Roadmap

Plan to bring `src/lib/components/ui/*` into alignment with the authoritative spec in `SKILL.md`. Ordered in suggested execution phases; each phase is independently shippable.

Scope: primitives only. Feature code migrations (SettingsDialog raw inputs, session rows, etc.) come *after* the primitives they depend on are correct.

## Status (2026-04-19)

Phases 1–6 and 7a/7b/7c/7d **landed**:
- Literal `text-[…]` → named scale across all primitives.
- Hand-rolled popover `shadow-[…]` → `shadow-popover`; kbd → `shadow-kbd-inset`.
- `focus-visible:shadow-focus-ring` rolled out to Button, Tabs.Trigger, DropdownMenu.Item, ContextMenu.Item, TabButton, Checkbox, and the titlebar palette trigger.
- New primitive: `Checkbox`. The single-consumer palette trigger now lives in `TitleBar.svelte`, not `ui/`. `IconButton` gained `shape: 'rounded' | 'flat'` + `tone: 'default' | 'danger'`.
- `TabBeam` accepts only horizontal `position: 'top' | 'bottom'`; `TabButton` lost `beamVariant` (status dots carry state).
- TitleBar live-reads `getCommandShortcut('toggle-command-palette')` and `getCommandShortcut('open-folder')` for palette and browser hints.
- SettingsDialog migrated to `<Input>` + `<Checkbox>`. TitleBar/WindowControls migrated to `<IconButton shape="flat">`.
- Legacy `.btn-sm`, `.field-input`, `.btn-ghost` removed from `src/app.css`. Unused `ui/menubar/` folder deleted.

Still deferred: Phase 4d (Radio/Switch/Select — no consumer yet), deeper SettingsDialog refactor (draft-hydration `$effect` → explicit hydrate function).

---

## Current inventory

Primitives that exist today under `src/lib/components/ui/`:

| Folder | Files | State |
|---|---|---|
| `alert/` | `alert.svelte`, `alert-description.svelte`, `alert-title.svelte` | Uses `text-[0.75rem]`, `text-[0.7rem]`, `text-[0.72rem]`. **Migrate to named scale.** |
| `badge/` | `badge.svelte` | `text-xs`; variants `default`, `success`, `warning`, `danger`, `accent`. Neutral badges use the default, not a `muted` alias. |
| `blur-fade/` | `blur-fade.svelte` | CSS stage entrance: fixed 6px blur and upward Y motion; `duration=0.4`, `offset=8`, `delay=0`. Headers retain `duration=0.5`, `offset=10`; no direction/blur props. |
| `breadcrumb/` | `breadcrumb.svelte`, `breadcrumb-item.svelte`, `breadcrumb-page.svelte`, `breadcrumb-separator.svelte` | `Breadcrumb` owns `nav` + `ol`; native ordered-list attrs and class forward to `ol`. Items and decorative separators render `li`; separator has `role="presentation"` + `aria-hidden="true"`. |
| `button/` | `button.svelte`, `icon-button.svelte` | Size variants use `text-[0.82rem]`, `text-[0.72rem]`, `text-[0.68rem]`. **Migrate.** Missing focus-ring. IconButton needs `shape` variant for flat chrome. |
| `button-group/` | `button-group.svelte` | Horizontal-only `ButtonGroup`; forwards native attributes and preserves `role="group"`. |
| `chrome-tabs/` | `tab-strip.svelte`, `tab-button.svelte` | `pane` variant still uses `text-[0.75rem]` → `text-sm`. `TabBeam` variant prop drops `warning/success/danger` (dots carry state). |
| `command/` | command components, palette dialog/search/footer | Named scale. `PaletteDialog` imports Bits UI directly inside `ui/`; features use the palette wrapper. |
| `context-menu/` | content, item, separator | Shared `menu-recipe.ts` owns content, item tone/disabled states and separator classes. |
| `dialog/` | content, description, footer, header, overlay, title | Public root and styled parts only; content forwards dismissal policies and `aria-busy`. |
| `dropdown-menu/` | content, item, separator | Shares `menu-recipe.ts` with context menus; no unused submenu/group exports. |
| `input/` | `input.svelte`, `textarea.svelte` | Textarea uses `text-[0.75rem]`. Migrate. |
| `kbd/` | `kbd.svelte`, `kbd-group.svelte` | Named scale and `shadow-kbd-inset`. `KbdGroup` groups caller-supplied chips and separators; it does not parse shortcuts. |
| `magic-card/` | `magic-card.svelte` | Keep — new-tab start options only. Cursor gradients fixed at 200px, spotlight uses `raised` at 0.8 opacity; only gradient endpoints, disabled and click action remain tunable. |
| `menubar/` | 5 files | **Unused after AppMenuBar deletion.** Candidate for removal. |
| `particles/` | `particles.svelte` | Stage atmosphere only. Canvas reads `--color-accent` once on effect start; per-particle alpha uses `globalAlpha`. Defaults: quantity 75, size 0.5, staticity 40, ease 60; no color/velocity props or StageView pass-through tuning. |
| `resizable/` | `handle.svelte`, `pane-group.svelte` | Inventory check. |
| `progress/` | `progress-bar.svelte` | `ProgressBar`: fixed `h-1 w-12`, done/active/total segments, computed title and progressbar ARIA values; caller class may override styling. |
| `scroll-area/` | `scroll-area.svelte` | Vertical scrollbar only; keeps native prop forwarding. |
| `separator/` | `separator.svelte` | Inventory check. |
| `tab-beam.svelte` | single file | Horizontal top/bottom sweep only; local `BeamPosition` union, accent default with optional custom project color. No semantic color variants or vertical mode. |
| `tabs/` | `tabs-list.svelte`, `tabs-trigger.svelte` | `text-[0.72rem]`. Migrate. |
| `tooltip/` | `tooltip-content.svelte`, `InfoTooltip.svelte` | `text-[0.72rem]`, `text-[0.68rem]`, hand-rolled shadow → `shadow-popover`. |

### Missing primitives (needed)

Identified from feature-code gaps:

- **`checkbox/`** — `SettingsDialog` uses raw `<input type="checkbox" class="accent-accent">`. Needs a primitive with the spec-correct focus ring + label layout.
- **`radio/`** — no feature code uses radios yet but they're implied by settings expansion. Add when first consumer appears.
- **`switch/`** — same as radio. Toggles in settings.
- **`select/`** — provider picker and settings dropdowns would benefit. Currently a raw `<select>`.
- **`titlebar-icon-button/`** — the flat-chrome square icon button pattern is hand-rolled in three places (`TitleBar.svelte` Settings button, `TitleBarMenu.svelte` trigger, `WindowControls.svelte` trigger base). Extract via either a new primitive or a `shape: 'flat'` variant on existing `IconButton`.

---

## Phase 1 — Literal → named scale migration

No behavioral change. Just swap every `text-[…]` literal in `ui/*` for the matching named scale.

| File | Change |
|---|---|
| `alert/alert.svelte` | `text-[0.75rem]` → `text-sm` |
| `alert/alert-description.svelte` | `text-[0.7rem]` → `text-xs` |
| `alert/alert-title.svelte` | `text-[0.72rem]` → `text-sm` |
| `badge/badge.svelte` | `text-[0.68rem]` → `text-xs` |
| `button/button.svelte` | `default: text-[0.82rem] → text-base`, `sm: text-[0.72rem] → text-sm`, `xs: text-[0.68rem] → text-xs` |
| `button/icon-button.svelte` | tooltip shortcut kbd `text-[0.68rem]` → `text-xs` |
| `chrome-tabs/tab-button.svelte` | pane variant `text-[0.75rem]` → `text-sm` |
| `command/` (all) | `text-[0.8rem]` → `text-base` (13.1px is closer to the intended ~13px reading size than `text-md`'s 13.6px) |
| `context-menu/context-menu-content.svelte` | `text-[0.8rem]` → `text-base` |
| `dropdown-menu/dropdown-menu-content.svelte` | `text-[0.8rem]` → `text-base` |
| `input/textarea.svelte` | `text-[0.75rem]` → `text-sm` |
| `kbd/kbd.svelte` | `text-[0.72rem]` → `text-sm` |
| `menubar/menubar-content.svelte` | `text-[0.8rem]` → `text-base` (if keeping menubar) |
| `menubar/menubar-sub-content.svelte` | `text-[0.8rem]` → `text-base` |
| `menubar/menubar-trigger.svelte` | `text-[0.75rem]` → `text-sm` |
| `tabs/tabs-trigger.svelte` | `text-[0.72rem]` → `text-sm` |
| `tooltip/tooltip-content.svelte` | `text-[0.72rem]` → `text-sm` |
| `tooltip/InfoTooltip.svelte` | `text-[0.68rem]` → `text-xs` |

After this phase: `grep text-\[ src/lib/components/ui/` returns zero results.

**Regression risk.** The scale shifted smaller across the board. Visual QA: scroll through the app with devtools, verify chrome reads at the intended sizes. `text-base` is now 0.82rem (was 1rem default). Buttons visibly tighter.

---

## Phase 2 — Shadow replacement

Replace hand-rolled `shadow-[…]` with named tokens.

| File | From | To |
|---|---|---|
| `dropdown-menu/dropdown-menu-content.svelte` | `shadow-[0_8px_24px_rgba(0,0,0,0.4)]` | `shadow-popover` |
| `context-menu/context-menu-content.svelte` | `shadow-[0_8px_24px_rgba(0,0,0,0.4)]` | `shadow-popover` |
| `menubar/menubar-content.svelte` | `shadow-[0_8px_24px_rgba(0,0,0,0.4)]` | `shadow-popover` |
| `menubar/menubar-sub-content.svelte` | `shadow-[0_8px_24px_rgba(0,0,0,0.4)]` | `shadow-popover` |
| `tooltip/tooltip-content.svelte` | `shadow-[0_10px_30px_rgba(0,0,0,0.45)]` | `shadow-popover` |
| `kbd/kbd.svelte` | `shadow-[inset_0_-1px_0_var(--color-edge)]` | `shadow-kbd-inset` |
| `dialog/dialog-content.svelte` | `shadow-[0_10px_30px_rgba(0,0,0,0.45)]` | `shadow-popover` |
| `CommandCenter.svelte` (feature) | `shadow-[0_16px_48px_rgba(0,0,0,0.5)]` | `shadow-popover` |

After: `grep 'shadow-\[' src/` returns only intentional one-offs (if any remain, justify them in a comment).

---

## Phase 3 — Focus ring rollout

Currently, most buttons have no visible focus state. The Input primitive does border-swap. Add `focus-visible:shadow-focus-ring` to:

| File | Notes |
|---|---|
| `button/button.svelte` | Add to base: `focus-visible:shadow-focus-ring focus-visible:outline-none`. |
| `tabs/tabs-trigger.svelte` | Same. |
| `dropdown-menu/dropdown-menu-item.svelte` | Replace `focus:bg-surface` alone with `focus:bg-surface focus-visible:shadow-focus-ring`. |
| `context-menu/context-menu-item.svelte` | Same. |
| `menubar/menubar-trigger.svelte` | Same. |
| `chrome-tabs/tab-button.svelte` | Already has `focus-visible:outline-*`. Swap to `focus-visible:shadow-focus-ring`. |

Input / Textarea stay on border-swap — that's the spec (§8.1).

---

## Phase 4 — Missing wrappers

### 4b · `TitlebarIconButton`

Three-way duplication in `TitleBar.svelte`, `TitleBarMenu.svelte`, `WindowControls.svelte`. Options:

**Option A — New primitive** at `src/lib/components/ui/button/titlebar-icon-button.svelte`:

```svelte
<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import type { Snippet } from 'svelte'

  let {
    tooltip,
    tooltipSide = 'bottom',
    danger = false,
    class: className,
    children,
    onclick,
    ...rest
  }: {
    tooltip?: string
    tooltipSide?: 'top' | 'right' | 'bottom' | 'left'
    danger?: boolean
    class?: string
    children?: Snippet
    onclick?: (e: MouseEvent) => void
  } = $props()

  const base =
    'flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-none border-none bg-transparent text-muted transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none'
  const hover = danger
    ? 'hover:bg-danger/15 hover:text-danger-bright'
    : 'hover:bg-raised hover:text-bright'
</script>

{#if tooltip}
  <TooltipRoot>
    <TooltipTrigger class={cn(base, hover, className)} aria-label={tooltip} {onclick} {...rest}>
      {#if children}{@render children()}{/if}
    </TooltipTrigger>
    <TooltipContent side={tooltipSide}>{tooltip}</TooltipContent>
  </TooltipRoot>
{:else}
  <button type="button" class={cn(base, hover, className)} {onclick} {...rest}>
    {#if children}{@render children()}{/if}
  </button>
{/if}
```

**Option B — Variant on existing `IconButton`** — add `shape: 'rounded' | 'flat'` (default `rounded`) and `danger: boolean`. Cleaner namespace (one button primitive, many shapes).

Recommend **Option B**. After it lands, refactor `TitleBar.svelte`, `TitleBarMenu.svelte`, and `WindowControls.svelte` to consume it. Drop local `buttonBase` / `triggerBase` consts.

### 4c · `Checkbox`

Add `src/lib/components/ui/checkbox/checkbox.svelte` using `bits-ui` `Checkbox.Root`. Wrap with the spec-correct focus ring, accent-color swatch, and label snippet. Target consumer: `SettingsDialog.svelte` — 6 raw native checkboxes there.

### 4d · `Radio`, `Switch`, `Select`

Defer until a concrete consumer exists. Not blocking.

---

## Phase 5 — Titlebar palette trigger

`TitleBar.svelte` owns its palette button markup and `data-slot="command-pill"`. It retains the 26px height, raised surface, rounded-md corners, focus ring, and `Kbd`/`KbdGroup` hint. No shared primitive is needed for this single consumer.

Palette hints use `getCommandShortcut('toggle-command-palette')` from `shortcuts/registry.svelte` + `splitShortcut()` from `shortcuts/spec`; the browser button uses `getCommandShortcut('open-folder')`. Both update after a user rebinds.

---

## Phase 6 — `TabBeam` simplification

Session state lives on status dots, not beam colors. `TabBeam` retains the accent default and custom project-path color.

- Semantic color variants and `TabButton.beamVariant` are removed.
- Local `BeamPosition = 'top' | 'bottom'` selects the horizontal edge; no backend type dependency.
- Horizontal sweep lives directly on `::after`; no orientation attribute or vertical animation remains.
- `class` overrides remain for loading-track height/rounding.

---

## Phase 7 — Feature-code cleanup

Once primitives above land, the remaining refactor is feature-side:

### 7a · `SettingsDialog.svelte`

- Swap 5 raw `<input class="field-input">` for `<Input>` primitive.
- Swap 6 native `<input type="checkbox">` for the new `<Checkbox>` primitive.
- Remove `updateProviderDraft` spread pattern; direct mutation on `$state` is idiomatic Svelte 5.
- Replace `$effect` draft-hydration with an explicit `hydrateDraft()` function called after `loadSettings()`.

### 7b · `TitleBar.svelte`, `TitleBarMenu.svelte`, `WindowControls.svelte`

- Delete local `buttonBase` / `triggerBase` consts.
- Adopt `<IconButton shape="flat">` (or `<TitlebarIconButton>`).
- Add tooltips to Settings and Menu triggers (currently missing).

### 7c · Utility class retirement

After Phase 7a, `.field-input` has no consumer. Remove from `src/app.css`. Same for `.btn-sm` and `.btn-ghost` once feature audits finish — grep first, they may still be used elsewhere.

### 7d · Remove unused menubar primitives

`src/lib/components/ui/menubar/*` is unused after AppMenuBar deletion. Either delete the whole folder or leave a `// UNUSED:` comment. Preference: delete; re-add only with a concrete consumer.

---

## Execution order

Recommended:

1. **Phase 1** (literal → named scale) — mechanical, no behavior change, large blast radius but easy to verify.
2. **Phase 2** (shadow replacement) — trivial.
3. **Phase 3** (focus rings) — adds keyboard-accessibility we're currently missing.
4. **Phase 4b** (IconButton shape variant) — unblocks titlebar cleanup.
5. **Phase 5** (titlebar palette trigger) + live shortcut read.
6. **Phase 6** (TabBeam simplification).
7. **Phase 4c** (Checkbox primitive) — unblocks SettingsDialog.
8. **Phase 7** (feature-code cleanup) — finishes the story.

Phases 1–3 should ship as one pass (call it "scale + shadow + focus"). Each subsequent phase is a standalone PR.

---

## Validation per phase

After every phase:

- `bun run check` — 0 errors / 0 warnings.
- `bun run build` — success.
- Visual spot-check:
  - Command palette opens with correct sizing.
  - Project tab active state reads with beam at bottom.
  - Dialog content elevates above backdrop.
  - Kbd chips in tooltips/palette look correct.
  - Status dots animate on working sessions.

No automated visual regression test exists. If one is added later, this roadmap is a natural place to pin baselines.

---

## Known non-goals

- **Storybook / Histoire.** Not proposed here. Separate decision.
- **Themes beyond dark.** Sworm is dark-only. Light mode is out of scope.
- **Accessibility audit beyond focus rings.** A pass for screen-reader labels, contrast ratios, and reduced-motion preferences is a separate initiative.
