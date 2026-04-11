<script lang="ts">
	import { onMount } from 'svelte';
	import LeftSidebar from '$lib/components/LeftSidebar.svelte';
	import ProjectMainView from '$lib/components/ProjectMainView.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import {
		loadProjects,
		getActiveProject,
		getActiveProjectId
	} from '$lib/stores/projects.svelte';
	import { loadProviders } from '$lib/stores/providers.svelte';

	onMount(() => {
		loadProjects();
		loadProviders();
	});

	let activeProject = $derived(getActiveProject());
</script>

<div class="flex flex-1 h-screen overflow-hidden">
	<LeftSidebar />

	<main class="flex flex-col flex-1 min-w-0 overflow-hidden">
		{#if activeProject}
			<ProjectMainView project={activeProject} />
		{:else}
			<EmptyState />
		{/if}
	</main>
</div>
