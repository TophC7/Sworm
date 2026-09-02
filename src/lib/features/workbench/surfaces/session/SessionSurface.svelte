<script lang="ts">
  import type { SessionTab } from '$lib/features/workbench/model'
  import SessionTerminal from '$lib/features/workbench/surfaces/session/SessionTerminal.svelte'

  let { tab }: { tab: SessionTab } = $props()
</script>

<SessionTerminal {tab} />
