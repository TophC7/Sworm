<script lang="ts">
	import TitleBar from '$lib/components/TitleBar.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import ProjectView from '$lib/components/ProjectView.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import SettingsDialog from '$lib/components/SettingsDialog.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { backend } from '$lib/api/backend';
	import { getActiveProject, addProject } from '$lib/stores/projects.svelte';
	import { openProject } from '$lib/stores/workspace.svelte';
	import { getZoomLevel } from '$lib/stores/ui.svelte';

	let activeProject = $derived(getActiveProject());
	let zoom = $derived(getZoomLevel());

	let settingsOpen = $state(false);
	let projectError = $state<string | null>(null);

	// Root-level zoom effect — lives here so it persists across all views
	$effect(() => {
		document.documentElement.style.fontSize = `${zoom * 100}%`;
		return () => { document.documentElement.style.fontSize = ''; };
	});

	async function handleNewProject() {
		try {
			const path = await backend.projects.selectDirectory();
			if (path) {
				const project = await addProject(path);
				openProject(project.id);
			}
		} catch (e) {
			projectError = `Failed to add project:\n${e}`;
		}
	}
</script>

<div class="flex flex-col h-screen overflow-hidden">
	<TitleBar
		onNewProject={handleNewProject}
		onSettings={() => (settingsOpen = true)}
	/>

	<main class="flex-1 flex flex-col min-h-0 overflow-hidden">
		{#if activeProject}
			<ProjectView project={activeProject} />
		{:else}
			<EmptyState />
		{/if}
	</main>

	<StatusBar />
</div>

<SettingsDialog open={settingsOpen} onClose={() => (settingsOpen = false)} />

{#if projectError}
	<ConfirmDialog
		open={true}
		title="Project Error"
		message={projectError}
		confirmLabel="Close"
		showCancel={false}
		onCancel={() => (projectError = null)}
		onConfirm={() => (projectError = null)}
	/>
{/if}
