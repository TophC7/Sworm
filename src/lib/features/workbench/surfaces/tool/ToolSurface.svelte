<script lang="ts">
  import NotificationTestView from '$lib/features/workbench/surfaces/tool/NotificationTestView.svelte'
  import type { ToolTab } from '$lib/features/workbench/state.svelte'

  let {
    tab = null
  }: {
    tab: ToolTab | null
  } = $props()
</script>

{#if tab?.tool === 'notification-test'}
  <NotificationTestView />
{/if}
