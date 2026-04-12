<script lang="ts">
	import type { Project, DiffContext } from '$lib/types/backend';
	import {
		getActiveSession,
		getSessions,
		updateSessionInList
	} from '$lib/stores/sessions.svelte';
	import { loadSessions } from '$lib/stores/sessions.svelte';
	import { startGitPolling, stopGitPolling, getGitSummary, refreshGit } from '$lib/stores/git.svelte';
	import SecondaryTabBar from '$lib/components/SecondaryTabBar.svelte';
	import SessionTerminal from '$lib/components/SessionTerminal.svelte';
	import GitSidebar from '$lib/components/GitSidebar.svelte';
	import { getGitSidebarWidth, setGitSidebarWidth, isGitSidebarCollapsed } from '$lib/stores/ui.svelte';
	import DiffViewer from '$lib/components/DiffViewer.svelte';
	import NewSessionView from '$lib/components/NewSessionView.svelte';

	let {
		project
	}: {
		project: Project;
	} = $props();

	let activeSession = $derived(getActiveSession());
	let sessions = $derived(getSessions());
	let liveSessions = $derived(sessions.filter((s) => s.status === 'running'));
	let gitSummary = $derived(getGitSummary(project.id));

	// View state: terminal, diff, or new-session picker
	let activeDiff = $state<{ filePath: string; context: DiffContext } | null>(null);
	let diffTemporary = $state(true);
	let diffError = $state<string | null>(null);
	let mainView = $state<'terminal' | 'diff' | 'new-session'>('terminal');

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

	// Reset state on project switch
	$effect(() => {
		project.id;
		activeDiff = null;
		diffError = null;
		mainView = 'terminal';
	});

	function handleViewDiff(filePath: string, context: DiffContext | null) {
		diffError = null;
		if (!context?.raw_diff) {
			activeDiff = null;
			mainView = 'terminal';
			return;
		}
		activeDiff = { filePath, context };
		diffTemporary = true;
		mainView = 'diff';
	}

	function handlePersistDiff() {
		diffTemporary = false;
	}

	function closeDiff() {
		activeDiff = null;
		mainView = 'terminal';
	}

	function handleDiffError(message: string | null) {
		diffError = message;
		if (message) {
			activeDiff = null;
			mainView = 'terminal';
		}
	}

	function handleRefreshGit() {
		void refreshGit(project.id, project.path);
	}

	function handleSelectDiff() {
		if (activeDiff) {
			mainView = 'diff';
		}
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
			activeDiffFile={activeDiff?.filePath}
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
		<!-- Secondary tab bar (sessions + diff) -->
		<SecondaryTabBar
			onNewSession={() => (mainView = 'new-session')}
			onSelectSession={() => (mainView = 'terminal')}
			activeDiffFile={activeDiff?.filePath}
			{diffTemporary}
			onSelectDiff={handleSelectDiff}
			onCloseDiff={closeDiff}
			onPersistDiff={handlePersistDiff}
		/>

		<!-- Content area -->
		<div class="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
			{#if diffError}
				<div class="border-b border-danger-border bg-danger-bg px-3 py-2 text-[0.76rem] text-danger-bright">
					{diffError}
				</div>
			{/if}

			{#if mainView === 'new-session'}
				<!-- New session picker -->
				<NewSessionView onCreated={() => (mainView = 'terminal')} />
			{:else if mainView === 'diff' && activeDiff}
				<!-- Diff view -->
				<DiffViewer
					rawDiff={activeDiff.context.raw_diff}
					filePath={activeDiff.filePath}
					oldContent={activeDiff.context.old_content}
					newContent={activeDiff.context.new_content}
					onClose={closeDiff}
				/>
			{:else}
				<!-- Terminal view -->
				<div class="flex-1 flex flex-col min-w-0">
					{#if activeSession}
						<SessionTerminal
							session={activeSession}
							onStatusChange={(status) => {
								updateSessionInList(activeSession.id, { status });
								if (status === 'exited' || status === 'stopped') {
									handleRefreshGit();
								}
							}}
						/>
					{:else}
						<NewSessionView onCreated={() => (mainView = 'terminal')} />
					{/if}
				</div>
			{/if}
		</div>
	</div>
</div>
