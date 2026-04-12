<script lang="ts">
	import { backend } from '$lib/api/backend';
	import { addProject } from '$lib/stores/projects.svelte';
	import { openProject } from '$lib/stores/workspace.svelte';
	import { Button } from '$lib/components/ui/button';
	import { BlurFade } from '$lib/components/ui/blur-fade';
	import StageView from '$lib/components/StageView.svelte';
	import FolderOpen from '@lucide/svelte/icons/folder-open';

	async function handleOpen() {
		try {
			const path = await backend.projects.selectDirectory();
			if (path) {
				const project = await addProject(path);
				openProject(project.id);
			}
		} catch (e) {
			alert(`Failed to add project: ${e}`);
		}
	}
</script>

<StageView>
	<div class="text-center">
		<BlurFade delay={0.1} duration={0.6} direction="up" offset={12}>
			<h1 class="text-4xl text-bright m-0 mb-2">ADE</h1>
		</BlurFade>

		<BlurFade delay={0.25} duration={0.6} direction="up" offset={10}>
			<p class="text-muted m-0 mb-6">Linux-first desktop app for coding-agent CLIs</p>
		</BlurFade>

		<BlurFade delay={0.4} duration={0.5} direction="up" offset={8}>
			<Button
				class="px-6 py-2.5 text-[0.95rem] bg-success/20 text-success hover:bg-success/30 border-none"
				onclick={handleOpen}
			>
				<FolderOpen size={16} />
				Open Repository
			</Button>
		</BlurFade>

		<BlurFade delay={0.55} duration={0.5} direction="up" offset={6}>
			<p class="text-[0.8rem] text-muted mt-4">Select a local git repository to get started</p>
		</BlurFade>
	</div>
</StageView>
