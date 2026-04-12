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

<!-- Rail: shown when collapsed in icon mode, clicking expands -->
{#if !sidebar.open && sidebar.collapsible === 'icon'}
  <button
    class={cn(
      'flex h-full w-full cursor-pointer flex-col items-center border-none bg-transparent px-2 pt-3',
      'transition-colors hover:bg-surface',
      className
    )}
    onclick={() => sidebar.toggle()}
    title="Expand sidebar"
    data-sidebar="rail"
  >
    {#if children}{@render children()}{/if}
  </button>
{/if}
