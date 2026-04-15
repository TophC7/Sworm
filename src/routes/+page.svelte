<script lang="ts">
  import { onMount } from 'svelte'
  import EmptyState from '$lib/components/EmptyState.svelte'
  import ProjectView from '$lib/components/ProjectView.svelte'
  import { getActiveProject, loadProjects } from '$lib/stores/projects.svelte'
  import { loadProviders } from '$lib/stores/providers.svelte'

  let activeProject = $derived(getActiveProject())

  onMount(() => {
    loadProjects()
    loadProviders()
  })
</script>

{#if activeProject}
  <ProjectView project={activeProject} />
{:else}
  <EmptyState />
{/if}
