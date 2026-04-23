<!--
  @component
  TaskSurface — host for a single task PTY run.

  Mounts xterm inside its root, spawns the task on mount, and rebinds
  to a fresh terminal whenever `tab.runId` changes (singleton restart).
  Reports lifecycle transitions back into the tab model via the tasks
  service so the tab title/status badge stay in sync.

  @param tab - the TaskTab describing this run
  @param projectId - the project whose `.sworm/tasks.json` defines the task
-->

<script lang="ts">
  import '@xterm/xterm/css/xterm.css'
  import { onDestroy } from 'svelte'
  import type { TaskTab } from '$lib/features/workbench/model'
  import { findTask } from '$lib/features/tasks/state.svelte'
  import { reportTaskStatus } from '$lib/features/tasks/service.svelte'
  import * as taskRegistry from '$lib/features/tasks/taskRegistry'

  let {
    tab,
    projectId
  }: {
    tab: TaskTab
    projectId: string
  } = $props()

  let containerEl = $state<HTMLDivElement | null>(null)
  let attachedRunId: string | null = null

  $effect(() => {
    const runId = tab.runId
    const container = containerEl
    if (!container || runId === attachedRunId) return

    const def = findTask(projectId, tab.taskId)
    if (attachedRunId && attachedRunId !== runId) {
      taskRegistry.detach(attachedRunId)
    }

    const terminal = taskRegistry.attach(
      {
        runId,
        projectId,
        taskId: tab.taskId,
        activeFilePath: tab.activeFilePath,
        clearBeforeStart: def?.clearOnRerun ?? false,
        onStatusChange: (status, exitCode) => reportTaskStatus(projectId, tab.id, status, exitCode)
      },
      container
    )

    attachedRunId = runId
    if (!terminal.hasStarted()) {
      terminal.start().catch((err) => {
        console.error('Task start failed:', err)
      })
    }
    terminal.focus()
  })

  onDestroy(() => {
    if (attachedRunId) {
      taskRegistry.detach(attachedRunId)
    }
  })
</script>

<div class="relative h-full min-h-0 w-full overflow-hidden bg-ground">
  <div bind:this={containerEl} class="absolute inset-0"></div>
</div>
