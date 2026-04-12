<script lang="ts">
	import { getProjects } from '$lib/stores/projects.svelte';
	import { openProject, getOpenProjectIds } from '$lib/stores/workspace.svelte';
	import {
		DropdownMenuRoot,
		DropdownMenuTrigger,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuSub,
		DropdownMenuSubTrigger,
		DropdownMenuSubContent
	} from '$lib/components/ui/dropdown-menu';
	import Worm from '@lucide/svelte/icons/worm';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	let {
		onNewProject,
		onSettings,
		onAbout
	}: {
		onNewProject: () => void;
		onSettings: () => void;
		onAbout?: () => void;
	} = $props();

	let projects = $derived(getProjects());
	let openIds = $derived(getOpenProjectIds());
	let recentProjects = $derived(projects.filter((p) => !openIds.includes(p.id)));
</script>

<DropdownMenuRoot>
	<DropdownMenuTrigger
		class="flex items-center justify-center w-8 h-8 bg-transparent border-none cursor-pointer text-accent hover:text-accent-bright transition-colors"
		title="Menu"
	>
		<Worm size={18} />
	</DropdownMenuTrigger>

	<DropdownMenuContent>
		<DropdownMenuItem onclick={onNewProject}>Open New Project</DropdownMenuItem>

		{#if recentProjects.length > 0}
			<DropdownMenuSub>
				<DropdownMenuSubTrigger class="w-full px-3 py-1.5 text-left cursor-pointer rounded-sm text-fg hover:bg-surface focus:bg-surface transition-colors outline-none flex items-center justify-between">
					Open Recents
					<ChevronRight size={12} class="text-muted" />
				</DropdownMenuSubTrigger>
				<DropdownMenuSubContent>
					{#each recentProjects as project (project.id)}
						<DropdownMenuItem onclick={() => openProject(project.id)}>
							<span class="truncate" title={project.path}>{project.name}</span>
						</DropdownMenuItem>
					{/each}
				</DropdownMenuSubContent>
			</DropdownMenuSub>
		{/if}

		<DropdownMenuSeparator />
		<DropdownMenuItem onclick={onSettings}>Settings</DropdownMenuItem>
		{#if onAbout}
			<DropdownMenuItem onclick={onAbout}>About</DropdownMenuItem>
		{/if}
	</DropdownMenuContent>
</DropdownMenuRoot>
