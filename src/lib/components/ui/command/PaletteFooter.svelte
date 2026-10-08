<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'

  let { class: className, children, ...rest }: HTMLAttributes<HTMLDivElement> = $props()
</script>

<div
  data-slot="palette-footer"
  class={cn(
    'flex shrink-0 items-center justify-between gap-4 border-t border-edge bg-surface/60 px-3 py-2 text-xs text-muted',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</div>
