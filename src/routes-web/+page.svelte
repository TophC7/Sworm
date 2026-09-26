<script lang="ts">
  import { WORKBENCH_PARAM } from '$lib/features/home/workbenchLink'
  import WorkbenchHost from './WorkbenchHost.svelte'

  // Read once: switching workbenches is always a full-page navigation. Without
  // an id this page becomes a new workbench; the server creates it on attach.
  const url = new URL(window.location.href)
  let workbenchId = url.searchParams.get(WORKBENCH_PARAM)
  if (!workbenchId) {
    workbenchId = crypto.randomUUID()
    url.searchParams.set(WORKBENCH_PARAM, workbenchId)
    history.replaceState(history.state, '', url)
  }
</script>

<WorkbenchHost {workbenchId} />
