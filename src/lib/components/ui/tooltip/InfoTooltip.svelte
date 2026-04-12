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
  import { buttonVariants } from '$lib/components/ui/button'
  import { TooltipContent, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
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

<TooltipRoot {delayDuration}>
  <TooltipTrigger
    type="button"
    aria-label={ariaLabel}
    class={cn(
      buttonVariants({ variant: 'ghost', size: 'icon-sm' }),
      'font-mono text-[0.68rem] font-semibold',
      className
    )}
  >
    ?
  </TooltipTrigger>
  <TooltipContent class={contentClass}>
    {#if children}{@render children()}{/if}
  </TooltipContent>
</TooltipRoot>
