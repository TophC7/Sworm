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
  let isOpen = $derived(sidebar.open)
</script>

<!--
  The Sidebar fills its container. Width is controlled by the parent
  (e.g. via inline style or store-driven value). This avoids double-width conflicts.
-->
<aside
  class={cn('flex h-full w-full flex-col overflow-hidden bg-ground', className)}
  data-sidebar="root"
  data-state={isOpen ? 'expanded' : 'collapsed'}
>
  {#if children}{@render children()}{/if}
</aside>
