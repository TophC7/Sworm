<script lang="ts">
  import { onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { backend } from '$lib/api/backend'
  import { preloadBuiltinCatalog } from '$lib/features/builtins/catalog'
  import EmptyState from '$lib/features/app-shell/project-picker/EmptyState.svelte'
  import ProjectView from '$lib/features/app-shell/ProjectView.svelte'
  import { addProject, getActiveProject, getProjects, loadProjects } from '$lib/features/projects/state.svelte'
  import { loadProviders } from '$lib/features/sessions/providers/state.svelte'
  import { openProject, restoreAppShellState } from '$lib/features/workbench/state.svelte'
  import { isProjectPickerOverride } from '$lib/features/app-shell/project-picker/state.svelte'
  import { describeClientError, logClientError } from '$lib/utils/client-error'

  let activeProject = $derived(getActiveProject())
  let showEmpty = $derived(isProjectPickerOverride() || !activeProject)
  let bootstrapError = $state<string | null>(null)

  // Drain the argv-supplied path (from Nautilus "Open With" or a CLI
  // invocation), add it as a project if needed, and activate it.
  // Safe to call any time: returns null when nothing is queued.
  async function consumePendingOpenPath() {
    try {
      const path = await backend.app.takePendingOpenPath()
      if (!path) return
      const project = await addProject(path)
      openProject(project.id)
    } catch (error) {
      logClientError('pending open-path failed', { error })
    }
  }

  // Order matters: projects must be loaded before app-shell restore so
  // we can validate persisted ids against the live project list and
  // skip projects the user has since deleted. The pending-open path
  // runs last so an explicit "Open in Sworm" always wins over the
  // persisted active project.
  onMount(() => {
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        await loadProjects()
        void loadProviders()
        await preloadBuiltinCatalog()
        const validIds = new Set(getProjects().map((project) => project.id))
        await restoreAppShellState(validIds)
        await consumePendingOpenPath()
      } catch (error) {
        bootstrapError = describeClientError(error)
        logClientError('startup bootstrap failed', {
          phase: '+page onMount',
          error,
          detail: bootstrapError
        })
      }
    })()

    // Second-instance launches fire this from the Rust single-instance
    // callback. The payload is empty; the backend has already stashed
    // the path in the pending slot.
    void listen('sworm://pending-open-changed', () => {
      void consumePendingOpenPath()
    }).then((fn) => {
      unlisten = fn
    })

    return () => unlisten?.()
  })
</script>

{#if bootstrapError}
  <div class="flex min-h-0 flex-1 items-center justify-center bg-ground p-6">
    <div class="max-w-3xl rounded-xl border border-danger-border bg-danger-bg/30 p-4 text-left">
      <h2 class="mb-2 text-sm font-semibold text-danger-bright">Startup failed</h2>
      <p class="mb-3 text-base text-danger">
        Open the devtools console and look for the logged `[sworm] startup bootstrap failed` details.
      </p>
      <pre class="overflow-auto text-sm whitespace-pre-wrap text-fg">{bootstrapError}</pre>
    </div>
  </div>
{:else if showEmpty}
  <EmptyState />
{:else if activeProject}
  <ProjectView project={activeProject} />
{/if}
