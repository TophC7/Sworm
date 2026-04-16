<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<h5 class={cn('text-[0.72rem] leading-snug font-semibold', className)}>
  {#if children}{@render children()}{/if}
</h5>
