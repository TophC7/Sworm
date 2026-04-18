<!--
  @component
  IconButton — ghost icon-only button with tooltip.

  Wraps TooltipRoot/Trigger/Content with Button(ghost, icon-sm) styling.
  Sets aria-label from the tooltip text for accessibility.
-->

<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import { buttonVariants } from './button.svelte'
  import TooltipContent from '../tooltip/tooltip-content.svelte'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    tooltip,
    shortcut,
    tooltipSide = 'bottom' as const,
    class: className,
    children,
    onclick,
    disabled = false
  }: {
    tooltip: string
    shortcut?: string
    tooltipSide?: 'top' | 'right' | 'bottom' | 'left'
    class?: string
    children?: Snippet
    onclick?: (e: MouseEvent) => void
    disabled?: boolean
  } = $props()
</script>

<Tooltip.Root>
  <Tooltip.Trigger
    class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), className)}
    aria-label={tooltip}
    {onclick}
    {disabled}
  >
    {#if children}{@render children()}{/if}
  </Tooltip.Trigger>
  <TooltipContent side={tooltipSide}>
    <span>{tooltip}</span>
    {#if shortcut}
      <kbd class="ml-2 font-mono text-[0.68rem] text-subtle">{shortcut}</kbd>
    {/if}
  </TooltipContent>
</Tooltip.Root>
