<script lang="ts">
  import { onMount } from 'svelte'
  import EmptyState from '$lib/components/EmptyState.svelte'
  import ProjectView from '$lib/components/ProjectView.svelte'
  import { getActiveProject, getProjects, loadProjects } from '$lib/stores/projects.svelte'
  import { loadProviders } from '$lib/stores/providers.svelte'
  import { restoreAppShellState } from '$lib/stores/workspace.svelte'
  import { isProjectPickerOverride } from '$lib/stores/ui.svelte'
  import { describeClientError, logClientError } from '$lib/utils/client-error'

  let activeProject = $derived(getActiveProject())
  let showEmpty = $derived(isProjectPickerOverride() || !activeProject)
  let bootstrapError = $state<string | null>(null)

  // Order matters: projects must be loaded before app-shell restore so
  // we can validate persisted ids against the live project list and
  // skip projects the user has since deleted.
  onMount(() => {
    void (async () => {
      try {
        await loadProjects()
        void loadProviders()
        const validIds = new Set(getProjects().map((project) => project.id))
        await restoreAppShellState(validIds)
      } catch (error) {
        bootstrapError = describeClientError(error)
        logClientError('startup bootstrap failed', {
          phase: '+page onMount',
          error,
          detail: bootstrapError
        })
      }
    })()
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
