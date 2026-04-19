<!--
  @component
  IconButton — icon-only ghost button with optional tooltip.

  Sizes:
    - size="sm" (default) — 20×20, for dense sidebars and rows.
    - size="md" — 28×28, for titlebar, activity bar, window controls.

  `tone="danger"` swaps hover to the danger palette — used for the
  window-close button. Non-danger instances stay on the accent hover.

  `active={true}` pins the hover treatment permanently — used for
  toggle-style buttons (activity bar view selector, toolbar pins, etc.).
  Consumers layer any extra active-marker chrome (e.g. the accent
  rail line) on top as a sibling element.

  `tooltip` is optional. Most icon buttons should have one — when a
  tooltip is supplied it also drives `aria-label`. Omit only when the
  button lives inside another tooltip (CommitTooltip's copy-hash) or
  a11y is covered by a nearby label; in that case pass `ariaLabel`.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  // Shared chrome for any square icon button — whether rendered as a
  // `<button>` via this primitive or composed into a bits-ui trigger
  // slot (DropdownMenuTrigger, Popover.Trigger, Tooltip.Trigger) that
  // needs the class string rather than a nested element.
  export const iconButtonVariants = tv({
    base: 'inline-flex items-center justify-center rounded-lg bg-transparent border-none text-muted cursor-pointer transition-colors hover:bg-surface hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
    variants: {
      size: {
        sm: 'w-5 h-5 p-0',
        md: 'w-7 h-7 p-0'
      },
      tone: {
        default: '',
        danger: 'hover:bg-danger/15 hover:text-danger-bright'
      },
      active: {
        true: 'bg-surface text-bright hover:bg-surface hover:text-bright',
        false: ''
      }
    },
    defaultVariants: {
      size: 'sm',
      tone: 'default',
      active: false
    }
  })

  export type IconButtonSize = VariantProps<typeof iconButtonVariants>['size']
  export type IconButtonTone = VariantProps<typeof iconButtonVariants>['tone']
</script>

<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import TooltipContent from '../tooltip/tooltip-content.svelte'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    tooltip,
    ariaLabel,
    shortcut,
    tooltipSide = 'bottom' as const,
    size = 'sm' as IconButtonSize,
    tone = 'default' as IconButtonTone,
    active,
    class: className,
    children,
    onclick,
    disabled = false
  }: {
    tooltip?: string
    ariaLabel?: string
    shortcut?: string
    tooltipSide?: 'top' | 'right' | 'bottom' | 'left'
    size?: IconButtonSize
    tone?: IconButtonTone
    active?: boolean
    class?: string
    children?: Snippet
    onclick?: (e: MouseEvent) => void
    disabled?: boolean
  } = $props()

  const classes = $derived(cn(iconButtonVariants({ size, tone, active }), className))
  // Only emit aria-label / aria-pressed when we actually have data for them.
  // Passing empty or undefined strings tells assistive tech "no accessible
  // name" / "this is a toggle in the off state" — both are misleading for
  // the non-toggle case where no tooltip or active flag was supplied.
  const label = $derived(ariaLabel ?? tooltip)
  const pressed = $derived(active === undefined ? undefined : active)
</script>

{#if tooltip}
  <Tooltip.Root>
    <Tooltip.Trigger class={classes} aria-label={label} aria-pressed={pressed} {onclick} {disabled}>
      {#if children}{@render children()}{/if}
    </Tooltip.Trigger>
    <TooltipContent side={tooltipSide}>
      <span>{tooltip}</span>
      {#if shortcut}
        <kbd class="ml-2 font-mono text-xs text-subtle">{shortcut}</kbd>
      {/if}
    </TooltipContent>
  </Tooltip.Root>
{:else}
  <button type="button" class={classes} aria-label={label} aria-pressed={pressed} {onclick} {disabled}>
    {#if children}{@render children()}{/if}
  </button>
{/if}
