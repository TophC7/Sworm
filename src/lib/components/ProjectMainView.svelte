<script lang="ts">
	import type { Project, GitSummary } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';
	import {
		getActiveSession,
		getSessions,
		updateSessionInList,
	} from '$lib/stores/sessions.svelte';
	import SessionTerminal from '$lib/components/SessionTerminal.svelte';
	import GitChangesPanel from '$lib/components/GitChangesPanel.svelte';

	let { project }: { project: Project } = $props();

	let gitSummary = $state<GitSummary | null>(null);
	let activeSession = $derived(getActiveSession());
	let sessions = $derived(getSessions());
	let liveSessions = $derived(sessions.filter((s) => s.status === 'running'));

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

	<!-- Main area: terminal + git panel -->
	<div class="flex-1 flex overflow-hidden min-h-0">
		<div class="flex-1 flex flex-col min-w-0">
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

		<div class="w-[280px] min-w-[200px] border-l border-edge overflow-y-auto">
			{#if gitSummary}
				<GitChangesPanel summary={gitSummary} projectPath={project.path} onRefresh={refreshGit} />
			{/if}
		</div>
	</div>
</div>
