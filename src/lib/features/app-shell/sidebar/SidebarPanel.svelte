<!--
  @component
  SidebarPanel — sidebar header, content, and collapse action.

  @param title - panel heading (uppercase label in the header)
  @param headerActions - snippet rendered right, before headerExtra (e.g. view action buttons)
  @param headerExtra - snippet rendered right, before collapse button (e.g. info tooltip)
  @param children - main content area
-->

<script lang="ts">
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { setSidebarCollapsed } from '$lib/features/app-shell/sidebar/state.svelte'
  import { PanelLeftClose } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let {
    title,
    headerActions,
    headerExtra,
    children
  }: {
    title: string
    headerActions?: Snippet
    headerExtra?: Snippet
    children?: Snippet
  } = $props()
</script>

<aside class="flex h-full flex-col bg-ground">
  <PanelHeader>
    {#snippet left()}
      <span class="truncate text-xs font-semibold tracking-wide text-muted uppercase" {title}>{title}</span>
    {/snippet}
    {#snippet right()}
      {#if headerActions}
        <div class="flex items-center gap-0.5">{@render headerActions()}</div>
      {/if}
      {@render headerExtra?.()}
      <IconButton tooltip="Collapse sidebar" onclick={() => setSidebarCollapsed(true)}>
        <PanelLeftClose size={12} />
      </IconButton>
    {/snippet}
  </PanelHeader>
  <div class="min-h-0 flex-1 overflow-hidden">
    {@render children?.()}
  </div>
</aside>
