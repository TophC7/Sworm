<!--
  @component
  InfoTooltip -- a reusable "?" button that shows contextual help in a tooltip.

  Pass tooltip body via the default children snippet.

  @param ariaLabel - accessible label for the trigger button
  @param class - optional extra classes on the trigger button
  @param contentClass - optional extra classes on the tooltip content panel
  @param delayDuration - hover delay before showing (default 150ms)
-->

<script lang="ts">
  import { Tooltip } from 'bits-ui'
  import { iconButtonVariants } from '../button'
  import TooltipContent from './tooltip-content.svelte'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    ariaLabel = 'More info',
    class: className,
    contentClass,
    delayDuration = 150,
    children
  }: {
    ariaLabel?: string
    class?: string
    contentClass?: string
    delayDuration?: number
    children?: Snippet
  } = $props()
</script>

<Tooltip.Root {delayDuration}>
  <Tooltip.Trigger
    type="button"
    aria-label={ariaLabel}
    class={cn(iconButtonVariants({ size: 'sm' }), 'font-mono text-xs font-semibold', className)}
  >
    ?
  </Tooltip.Trigger>
  <TooltipContent class={contentClass}>
    {#if children}{@render children()}{/if}
  </TooltipContent>
</Tooltip.Root>
