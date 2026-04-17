<script lang="ts">
  import { onMount } from 'svelte'
  import EmptyState from '$lib/components/EmptyState.svelte'
  import ProjectView from '$lib/components/ProjectView.svelte'
  import { getActiveProject, getProjects, loadProjects } from '$lib/stores/projects.svelte'
  import { loadProviders } from '$lib/stores/providers.svelte'
  import { restoreAppShellState } from '$lib/stores/workspace.svelte'
  import { isProjectPickerOverride } from '$lib/stores/ui.svelte'

  let activeProject = $derived(getActiveProject())
  let showEmpty = $derived(isProjectPickerOverride() || !activeProject)

  // Order matters: projects must be loaded before app-shell restore so
  // we can validate persisted ids against the live project list and
  // skip projects the user has since deleted.
  onMount(async () => {
    await loadProjects()
    void loadProviders()
    const validIds = new Set(getProjects().map((project) => project.id))
    await restoreAppShellState(validIds)
  })
</script>

{#if showEmpty}
  <EmptyState />
{:else if activeProject}
  <ProjectView project={activeProject} />
{/if}
