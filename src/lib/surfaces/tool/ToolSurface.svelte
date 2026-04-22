<script lang="ts">
  import NotificationTestView from '$lib/surfaces/tool/NotificationTestView.svelte'
  import type { ToolTab } from '$lib/workbench/state.svelte'

  let {
    tab
  }: {
    tab: ToolTab
  } = $props()
</script>

{#if tab.tool === 'notification-test'}
  <NotificationTestView />
{/if}
