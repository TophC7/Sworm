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
  <div
    class={cn(
      'flex h-8 min-h-8 shrink-0 items-center justify-between border-b border-edge bg-surface px-2.5',
      className
    )}
    data-sidebar="header"
  >
    {#if children}{@render children()}{/if}
  </div>
{/if}
