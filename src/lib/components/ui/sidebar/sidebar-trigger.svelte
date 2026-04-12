<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'
  import { Button } from '$lib/components/ui/button'
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

<Button
  variant="ghost"
  size="icon-sm"
  class={cn(className)}
  onclick={() => sidebar.toggle()}
  title={sidebar.open ? 'Collapse sidebar' : 'Expand sidebar'}
>
  {#if children}{@render children()}{/if}
</Button>
