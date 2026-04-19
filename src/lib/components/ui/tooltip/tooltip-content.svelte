<!--
  @component
  TooltipContent -- styled tooltip content wrapper for Sworm

  @param class - optional extra classes for the floating content
  @param sideOffset - gap between trigger and tooltip content
-->

<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    sideOffset = 8,
    side,
    align,
    ...rest
  }: {
    class?: string
    children?: Snippet
    sideOffset?: number
    side?: 'top' | 'right' | 'bottom' | 'left'
    align?: 'start' | 'center' | 'end'
  } = $props()
</script>

<Tooltip.Portal>
  <Tooltip.Content
    {sideOffset}
    {side}
    {align}
    class={cn(
      'z-50 max-w-80 rounded-lg border border-edge bg-overlay px-3 py-2 text-sm leading-5 text-fg shadow-popover',
      className
    )}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </Tooltip.Content>
</Tooltip.Portal>
