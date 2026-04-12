<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'
  import { useSidebar } from './sidebar-state.svelte'

  let {
    class: className,
    children
  }: {
    class?: string
    children?: Snippet
  } = $props()

  const sidebar = useSidebar()
</script>

{#if sidebar.open}
  <div class={cn('shrink-0 border-t border-edge', className)} data-sidebar="footer">
    {#if children}{@render children()}{/if}
  </div>
{/if}
