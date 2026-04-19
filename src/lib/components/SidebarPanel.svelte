<!--
  @component
  SidebarPanel — unified sidebar wrapper used by all sidebar views.

  Owns the header chrome (title, action slots, collapse button with tooltip)
  and a scrollable content area. No sidebar view should import PanelLeftClose
  or setSidebarCollapsed directly — this component is the single source for
  collapse behavior.

  @param title - panel heading (uppercase label in the header)
  @param headerActions - snippet rendered left, after title (e.g. refresh button)
  @param headerExtra - snippet rendered right, before collapse button (e.g. info tooltip)
  @param children - main content area
  @param class - optional extra classes on the outer container
-->

<script lang="ts">
  import PanelHeader from '$lib/components/PanelHeader.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { setSidebarCollapsed } from '$lib/stores/ui.svelte'
  import { PanelLeftClose } from '$lib/icons/lucideExports'
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
      <IconButton tooltip="Collapse sidebar" onclick={() => setSidebarCollapsed(true)}>
        <PanelLeftClose size={12} />
      </IconButton>
    {/snippet}
  </PanelHeader>
  <div class="min-h-0 flex-1 overflow-hidden">
    {#if children}{@render children()}{/if}
  </div>
</aside>
