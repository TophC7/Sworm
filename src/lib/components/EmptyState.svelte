<script lang="ts">
	import { backend } from '$lib/api/backend';
	import { addProject, selectProject } from '$lib/stores/projects.svelte';

	async function handleOpen() {
		try {
			const path = await backend.projects.selectDirectory();
			if (path) {
				const project = await addProject(path);
				selectProject(project.id);
			}
		} catch (e) {
			alert(`Failed to add project: ${e}`);
		}
	}
</script>

<div class="flex-1 flex items-center justify-center bg-ground">
	<div class="text-center">
		<h1 class="text-4xl text-bright m-0 mb-2">ADE</h1>
		<p class="text-muted m-0 mb-6">Linux-first desktop app for coding-agent CLIs</p>
		<button
			class="px-6 py-2.5 bg-success/20 text-success border-none rounded-md text-[0.95rem] cursor-pointer font-medium hover:bg-success/30 transition-colors"
			onclick={handleOpen}
		>
			Open Repository
		</button>
		<p class="text-[0.8rem] text-muted mt-4">Select a local git repository to get started</p>
	</div>
</div>
