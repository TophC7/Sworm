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

<div class="project-view">
	<!-- Top bar: project info + git status -->
	<header class="project-header">
		<div class="project-info">
			<h2 class="project-name">{project.name}</h2>
			{#if gitSummary?.branch}
				<span class="badge branch">{gitSummary.branch}</span>
			{/if}
			{#if gitSummary?.base_ref}
				<span class="badge base">← {gitSummary.base_ref}</span>
			{/if}
			{#if gitSummary && (gitSummary.ahead ?? 0) > 0}
				<span class="badge ahead">↑{gitSummary.ahead}</span>
			{/if}
			{#if gitSummary && (gitSummary.behind ?? 0) > 0}
				<span class="badge behind">↓{gitSummary.behind}</span>
			{/if}
		</div>

		<div class="project-stats">
			{#if liveSessions.length > 0}
				<span class="live-indicator" title="Live sessions in this project">
					● {liveSessions.length} live
				</span>
			{/if}
			{#if liveSessions.length > 1}
				<span class="shared-warning" title="Sessions share the same working tree">
					⚠ shared workspace
				</span>
			{/if}
			<span class="path" title={project.path}>{project.path}</span>
		</div>
	</header>

	<!-- Main area: terminal + git panel -->
	<div class="main-area">
		<div class="terminal-area">
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
				<div class="no-session">
					<p>Select or create a session from the sidebar</p>
				</div>
			{/if}
		</div>

		<div class="git-panel">
			{#if gitSummary}
				<GitChangesPanel summary={gitSummary} projectPath={project.path} onRefresh={refreshGit} />
			{/if}
		</div>
	</div>
</div>

<style>
	.project-view {
		flex: 1;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.project-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 14px;
		background: #161b22;
		border-bottom: 1px solid #30363d;
		min-height: 40px;
		gap: 12px;
	}

	.project-info {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}

	.project-name {
		margin: 0;
		font-size: 0.95rem;
		font-weight: 600;
		color: #f0f6fc;
		white-space: nowrap;
	}

	.badge {
		font-size: 0.7rem;
		padding: 1px 6px;
		border-radius: 10px;
		white-space: nowrap;
	}

	.branch {
		background: #1f6feb33;
		color: #58a6ff;
	}

	.base {
		background: #30363d;
		color: #8b949e;
	}

	.ahead {
		background: #23863633;
		color: #3fb950;
	}

	.behind {
		background: #da363333;
		color: #f85149;
	}

	.project-stats {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 0.72rem;
		color: #8b949e;
		flex-shrink: 0;
	}

	.live-indicator {
		color: #3fb950;
	}

	.shared-warning {
		color: #d29922;
	}

	.path {
		max-width: 200px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.main-area {
		flex: 1;
		display: flex;
		overflow: hidden;
		min-height: 0;
	}

	.terminal-area {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}

	.git-panel {
		width: 280px;
		min-width: 200px;
		border-left: 1px solid #30363d;
		overflow-y: auto;
	}

	.no-session {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		color: #484f58;
		font-size: 0.9rem;
	}
</style>
