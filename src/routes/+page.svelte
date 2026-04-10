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

<div class="workspace">
	<LeftSidebar />

	<main class="main-content">
		{#if activeProject}
			<ProjectMainView project={activeProject} />
		{:else}
			<EmptyState />
		{/if}
	</main>
</div>

<style>
	.workspace {
		display: flex;
		flex: 1;
		height: 100vh;
		overflow: hidden;
	}

	.main-content {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
		overflow: hidden;
	}
</style>
