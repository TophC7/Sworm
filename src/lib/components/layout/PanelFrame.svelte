<script lang="ts">
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    title,
    headerActions,
    headerExtra,
    children,
    class: className
  }: {
    title: string
    headerActions?: Snippet
    headerExtra?: Snippet
    children?: Snippet
    class?: string
  } = $props()
</script>

<aside class={cn('flex h-full flex-col bg-ground', className)}>
  <PanelHeader>
    {#snippet left()}
      <span class="text-xs font-semibold tracking-wide text-muted uppercase">{title}</span>
      {#if headerActions}{@render headerActions()}{/if}
    {/snippet}
    {#snippet right()}
      {#if headerExtra}{@render headerExtra()}{/if}
    {/snippet}
  </PanelHeader>
  <div class="min-h-0 flex-1 overflow-hidden">
    {#if children}{@render children()}{/if}
  </div>
</aside>
