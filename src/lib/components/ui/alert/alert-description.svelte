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

<div class={cn('text-xs leading-snug opacity-85', className)}>
  {#if children}{@render children()}{/if}
</div>
