<script lang="ts">
	import { onMount } from 'svelte';
	import LeftSidebar from '$lib/components/LeftSidebar.svelte';
	import ProjectMainView from '$lib/components/ProjectMainView.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { loadProjects, getActiveProject } from '$lib/stores/projects.svelte';
	import { loadProviders } from '$lib/stores/providers.svelte';
	import { createPanelResize } from '$lib/attachments/resizeHandle.svelte';

	onMount(() => {
		loadProjects();
		loadProviders();
	});

	let activeProject = $derived(getActiveProject());
	const leftResize = createPanelResize(240, 200, 420);
</script>

<div class="flex flex-1 h-screen overflow-hidden">
	<LeftSidebar width={leftResize.width} />
	<div
		class="w-1 shrink-0 cursor-col-resize bg-surface transition-colors hover:bg-accent/40"
		{@attach leftResize.handle((e) => e.clientX)}
		role="separator"
		aria-label="Resize project sidebar"
	></div>

	<main class="flex flex-col flex-1 min-w-0 overflow-hidden">
		{#if activeProject}
			<ProjectMainView project={activeProject} />
		{:else}
			<EmptyState />
		{/if}
	</main>
</div>
