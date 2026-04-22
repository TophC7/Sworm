<!--
  @component
  SidebarPanel — app-shell wrapper around the shared panel frame.

  Owns the sidebar-specific collapse action while delegating the header/body
  layout to the shared `PanelFrame` primitive.

  @param title - panel heading (uppercase label in the header)
  @param headerActions - snippet rendered left, after title (e.g. refresh button)
  @param headerExtra - snippet rendered right, before collapse button (e.g. info tooltip)
  @param children - main content area
  @param class - optional extra classes on the outer container
-->

<script lang="ts">
  import PanelFrame from '$lib/components/layout/PanelFrame.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { setSidebarCollapsed } from '$lib/features/app-shell/sidebar/state.svelte'
  import { PanelLeftClose } from '$lib/icons/lucideExports'
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

<PanelFrame {title} {headerActions} class={className}>
  {#snippet headerExtra()}
    {#if headerExtra}{@render headerExtra()}{/if}
    <IconButton tooltip="Collapse sidebar" onclick={() => setSidebarCollapsed(true)}>
      <PanelLeftClose size={12} />
    </IconButton>
  {/snippet}
  {#snippet children()}
    {#if children}{@render children()}{/if}
  {/snippet}
</PanelFrame>
