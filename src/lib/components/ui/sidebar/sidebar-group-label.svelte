<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    children
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<div
  class={cn('px-2.5 py-1 text-[0.68rem] font-medium tracking-wide text-muted uppercase', className)}
  data-sidebar="group-label"
>
  {#if children}{@render children()}{/if}
</div>
