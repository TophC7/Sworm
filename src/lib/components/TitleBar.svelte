<script lang="ts">
	import AppMenuBar from '$lib/components/AppMenuBar.svelte';
	import ProjectTabBar from '$lib/components/ProjectTabBar.svelte';
	import WindowControls from '$lib/components/WindowControls.svelte';
	import Worm from '@lucide/svelte/icons/worm';

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
	class="flex items-center bg-surface border-b border-edge shrink-0 min-h-9"
	data-tauri-drag-region
>
	<!-- Logo (static branding) -->
	<div class="shrink-0 flex items-center justify-center w-8 h-8 mx-1 text-accent">
		<Worm size={18} />
	</div>

	<!-- Menubar -->
	<div class="shrink-0">
		<AppMenuBar {onNewProject} {onSettings} {onAbout} />
	</div>

	<!-- Project tabs -->
	<ProjectTabBar onAddProject={onNewProject} />

	<WindowControls />
</header>
