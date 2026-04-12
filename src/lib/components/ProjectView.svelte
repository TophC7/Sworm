<script lang="ts">
	import type { Project, DiffContext } from '$lib/types/backend';
	import { loadSessions } from '$lib/stores/sessions.svelte';
	import { startGitPolling, stopGitPolling, getGitSummary, refreshGit } from '$lib/stores/git.svelte';
	import GitSidebar from '$lib/components/GitSidebar.svelte';
	import { getGitSidebarWidth, setGitSidebarWidth, isGitSidebarCollapsed } from '$lib/stores/ui.svelte';
	import PaneGrid from '$lib/components/PaneGrid.svelte';
	import {
		addDiffTab,
		getFocusedDiffTab,
		promoteTemporaryTab
	} from '$lib/stores/workspace.svelte';

	let {
		project
	}: {
		project: Project;
	} = $props();

	let gitSummary = $derived(getGitSummary(project.id));
	let focusedDiffTab = $derived(getFocusedDiffTab(project.id));
	let diffError = $state<string | null>(null);

	let sidebarCollapsed = $derived(isGitSidebarCollapsed());
	let sidebarWidth = $derived(getGitSidebarWidth());
	let layoutEl = $state<HTMLDivElement | null>(null);

	// Resize handle for git sidebar (left side)
	function sidebarResizeHandle(element: HTMLElement) {
		function onPointerDown(e: PointerEvent) {
			e.preventDefault();
			document.body.style.cursor = 'col-resize';
			document.body.style.userSelect = 'none';

			function onMove(e: PointerEvent) {
				if (!layoutEl) return;
				const w = e.clientX - layoutEl.getBoundingClientRect().left;
				setGitSidebarWidth(w);
			}
			function onUp() {
				document.body.style.cursor = '';
				document.body.style.userSelect = '';
				window.removeEventListener('pointermove', onMove);
				window.removeEventListener('pointerup', onUp);
			}
			window.addEventListener('pointermove', onMove);
			window.addEventListener('pointerup', onUp);
		}
		element.addEventListener('pointerdown', onPointerDown);
		return () => element.removeEventListener('pointerdown', onPointerDown);
	}

	// Load sessions and start git polling when project changes
	$effect(() => {
		const id = project.id;
		const path = project.path;

		loadSessions(id);
		startGitPolling(id, path);

		return () => {
			stopGitPolling(id);
		};
	});

	// Reset transient git UI state on project switch
	$effect(() => {
		project.id;
		diffError = null;
	});

	function handleViewDiff(filePath: string, context: DiffContext | null) {
		diffError = null;
		if (!context?.raw_diff) {
			return;
		}
		addDiffTab(project.id, filePath, context, true);
	}

	function handlePersistDiff() {
		if (focusedDiffTab?.temporary) {
			promoteTemporaryTab(focusedDiffTab.id);
		}
	}

	function handleDiffError(message: string | null) {
		diffError = message;
	}

	function handleRefreshGit() {
		void refreshGit(project.id, project.path);
	}
</script>

<!-- Horizontal layout: sidebar | tabs+content -->
<div class="flex-1 flex overflow-hidden min-h-0" bind:this={layoutEl}>
	<!-- Git sidebar (left, full height) -->
	<div
		class="shrink-0 border-r border-edge overflow-hidden"
		style={sidebarCollapsed ? '' : `width: ${sidebarWidth}px;`}
	>
		<GitSidebar
			summary={gitSummary}
			projectPath={project.path}
			onRefresh={handleRefreshGit}
			onViewDiff={handleViewDiff}
			activeDiffFile={focusedDiffTab?.filePath}
			onDiffError={handleDiffError}
			onPersistDiff={handlePersistDiff}
		/>
	</div>

	<!-- Sidebar resize handle -->
	{#if !sidebarCollapsed}
		<div
			class="w-1 shrink-0 cursor-col-resize bg-ground transition-colors hover:bg-accent/40"
			{@attach sidebarResizeHandle}
			role="separator"
			aria-label="Resize git sidebar"
		></div>
	{/if}

	<!-- Right column: session tabs + content -->
	<div class="flex-1 flex flex-col min-w-0 overflow-hidden">
		<div class="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
			{#if diffError}
				<div class="border-b border-danger-border bg-danger-bg px-3 py-2 text-[0.76rem] text-danger-bright">
					{diffError}
				</div>
			{/if}

			<PaneGrid projectId={project.id} projectPath={project.path} />
		</div>
	</div>
</div>
