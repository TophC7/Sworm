<script lang="ts">
  import type { Session } from '$lib/types/backend'
  import SessionTerminal from '$lib/features/workbench/surfaces/session/SessionTerminal.svelte'

  let {
    session,
    projectId,
    projectPath,
    locked = false,
    onStatusChange
  }: {
    session: Session
    projectId: string
    projectPath: string
    locked?: boolean
    onStatusChange?: (status: Session['status']) => void
  } = $props()
</script>

<SessionTerminal {session} {projectId} {projectPath} {locked} {onStatusChange} />
