<script lang="ts">
	import LogoMenu from '$lib/components/LogoMenu.svelte';
	import ProjectTabBar from '$lib/components/ProjectTabBar.svelte';

	let {
		onNewProject,
		onSettings,
		onAbout
	}: {
		onNewProject: () => void;
		onSettings: () => void;
		onAbout?: () => void;
	} = $props();
</script>

<header
	class="flex items-center bg-surface border-b border-edge shrink-0 min-h-9 pr-2"
	data-tauri-drag-region
>
	<div class="shrink-0 px-1">
		<LogoMenu {onNewProject} {onSettings} {onAbout} />
	</div>

	<ProjectTabBar onAddProject={onNewProject} />
</header>
