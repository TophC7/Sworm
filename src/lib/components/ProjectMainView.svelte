<script lang="ts">
	import type { Project, GitSummary, DiffContext } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';
	import {
		getActiveSession,
		getSessions,
		updateSessionInList
	} from '$lib/stores/sessions.svelte';
	import SessionTerminal from '$lib/components/SessionTerminal.svelte';
	import GitChangesPanel from '$lib/components/GitChangesPanel.svelte';
	import DiffViewer from '$lib/components/DiffViewer.svelte';
	import { createPanelResize } from '$lib/attachments/resizeHandle.svelte';

	let { project }: { project: Project } = $props();

	let gitSummary = $state<GitSummary | null>(null);
	let activeSession = $derived(getActiveSession());
	let sessions = $derived(getSessions());
	let liveSessions = $derived(sessions.filter((s) => s.status === 'running'));

	// Diff state — when set, the main area can show the diff viewer
	let activeDiff = $state<{ filePath: string; context: DiffContext } | null>(null);
	let diffError = $state<string | null>(null);
	let mainView = $state<'terminal' | 'diff'>('terminal');
	const gitResize = createPanelResize(280, 220, 520);
	let mainLayoutEl = $state<HTMLDivElement | null>(null);

	function handleViewDiff(filePath: string, context: DiffContext | null) {
		diffError = null;
		if (!context?.raw_diff) {
			activeDiff = null;
			mainView = 'terminal';
			return;
		}
		activeDiff = { filePath, context };
		mainView = 'diff';
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

	$effect(() => {
		if (!project?.path) {
			return;
		}

		refreshGit();

		const interval = setInterval(() => {
			if (liveSessions.length > 0) {
				refreshGit();
			}
		}, 10_000);

		return () => clearInterval(interval);
	});

	$effect(() => {
		project.id;
		activeDiff = null;
		diffError = null;
		mainView = 'terminal';
	});

	function refreshGit() {
		if (project?.path) {
			backend.git.getSummary(project.path).then((s) => {
				gitSummary = s;
			});
		}
	}
</script>

<div class="flex-1 flex flex-col overflow-hidden">
	<!-- Top bar: project info + git status -->
	<header class="flex items-center justify-between px-3.5 py-2 bg-surface border-b border-edge min-h-10 gap-3">
		<div class="flex items-center gap-2 min-w-0">
			<h2 class="m-0 text-[0.95rem] font-semibold text-bright whitespace-nowrap">{project.name}</h2>
			{#if gitSummary?.branch}
				<span class="text-[0.7rem] px-1.5 py-px rounded-[10px] whitespace-nowrap bg-accent-bg text-accent">
					{gitSummary.branch}
				</span>
			{/if}
			{#if gitSummary?.base_ref}
				<span class="text-[0.7rem] px-1.5 py-px rounded-[10px] whitespace-nowrap bg-edge text-muted">
					&larr; {gitSummary.base_ref}
				</span>
			{/if}
			{#if gitSummary && (gitSummary.ahead ?? 0) > 0}
				<span class="text-[0.7rem] px-1.5 py-px rounded-[10px] whitespace-nowrap bg-success-bg text-success">
					&uarr;{gitSummary.ahead}
				</span>
			{/if}
			{#if gitSummary && (gitSummary.behind ?? 0) > 0}
				<span class="text-[0.7rem] px-1.5 py-px rounded-[10px] whitespace-nowrap bg-danger-bg text-danger">
					&darr;{gitSummary.behind}
				</span>
			{/if}
		</div>

		<div class="flex items-center gap-2.5 text-[0.72rem] text-muted shrink-0">
			{#if liveSessions.length > 0}
				<span class="text-success" title="Live sessions in this project">
					&#9679; {liveSessions.length} live
				</span>
			{/if}
			{#if liveSessions.length > 1}
				<span class="text-warning" title="Sessions share the same working tree">
					&#9888; shared workspace
				</span>
			{/if}
			<span class="max-w-[200px] overflow-hidden text-ellipsis whitespace-nowrap" title={project.path}>
				{project.path}
			</span>
		</div>
	</header>

	<!-- Main area: terminal/diff + git sidebar -->
	<div class="flex-1 flex overflow-hidden min-h-0" bind:this={mainLayoutEl}>
		<!-- Main content area -->
		<div class="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
			{#if diffError}
				<div class="border-b border-danger-border bg-danger-bg px-3 py-2 text-[0.76rem] text-danger-bright">
					{diffError}
				</div>
			{/if}

			<!-- Tab bar — only visible when a diff is open -->
			{#if activeDiff}
				<div class="flex items-center bg-ground border-b border-edge px-1">
					<button
						class="px-3 py-1.5 text-[0.72rem] border-b-2 transition-colors {mainView === 'terminal' ? 'border-accent text-accent' : 'border-transparent text-muted hover:text-fg'}"
						onclick={() => (mainView = 'terminal')}
					>
						Terminal
					</button>
					<button
						class="px-3 py-1.5 text-[0.72rem] border-b-2 transition-colors flex items-center gap-1.5 {mainView === 'diff' ? 'border-accent text-accent' : 'border-transparent text-muted hover:text-fg'}"
						onclick={() => (mainView = 'diff')}
					>
						<span>Diff</span>
						<span class="text-[0.65rem] text-muted font-mono truncate max-w-[180px]">{activeDiff.filePath}</span>
					</button>
				</div>
			{/if}

			<!-- Terminal view (hidden when viewing diff) -->
			<div class="flex-1 flex flex-col min-w-0 {mainView === 'diff' && activeDiff ? 'hidden' : ''}">
				{#if activeSession}
					<SessionTerminal
						session={activeSession}
						onStatusChange={(status) => {
							updateSessionInList(activeSession.id, { status });
							if (status === 'exited' || status === 'stopped') {
								refreshGit();
							}
						}}
					/>
				{:else}
					<div class="flex-1 flex items-center justify-center text-subtle text-[0.9rem]">
						<p>Select or create a session from the sidebar</p>
					</div>
				{/if}
			</div>

			<!-- Diff view -->
			{#if activeDiff && mainView === 'diff'}
				<DiffViewer
					rawDiff={activeDiff.context.raw_diff}
					filePath={activeDiff.filePath}
					oldContent={activeDiff.context.old_content}
					newContent={activeDiff.context.new_content}
					onClose={closeDiff}
				/>
			{/if}
		</div>

		<div
			class="w-1 shrink-0 cursor-col-resize bg-ground transition-colors hover:bg-accent/40"
			{@attach gitResize.handle((e) => mainLayoutEl!.getBoundingClientRect().right - e.clientX)}
			role="separator"
			aria-label="Resize git sidebar"
		></div>

		<!-- Git sidebar -->
		<div
			class="min-w-[220px] max-w-[520px] shrink-0 border-l border-edge overflow-y-auto"
			style={`width: ${gitResize.width}px;`}
		>
			{#if gitSummary}
				<GitChangesPanel
					summary={gitSummary}
					projectPath={project.path}
					onRefresh={refreshGit}
					onViewDiff={handleViewDiff}
					activeDiffFile={activeDiff?.filePath}
					onDiffError={handleDiffError}
				/>
			{/if}
		</div>
	</div>
</div>
