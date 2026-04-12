<script lang="ts">
  import { onMount } from 'svelte'
  import AppShell from '$lib/components/AppShell.svelte'
  import { loadProjects } from '$lib/stores/projects.svelte'
  import { loadProviders } from '$lib/stores/providers.svelte'

  onMount(() => {
    loadProjects()
    loadProviders()
  })
</script>

<AppShell />
