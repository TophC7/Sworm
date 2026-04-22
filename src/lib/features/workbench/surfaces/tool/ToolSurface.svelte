<script lang="ts">
  import NotificationTestView from '$lib/features/workbench/surfaces/tool/NotificationTestView.svelte'
  import type { ToolTab } from '$lib/features/workbench/model'

  let {
    tab = null
  }: {
    tab: ToolTab | null
  } = $props()
</script>

{#if tab?.tool === 'notification-test'}
  <NotificationTestView />
{/if}
