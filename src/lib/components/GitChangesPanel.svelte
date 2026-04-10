<script lang="ts">
	import { onMount } from 'svelte';
	import type { GitChange, GitCommit, GitSummary } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';

	let {
		summary,
		projectPath,
		onRefresh
	}: {
		summary: GitSummary;
		projectPath: string;
		onRefresh?: () => void;
	} = $props();

	let selectedFile = $state<string | null>(null);
	let diffContent = $state<string | null>(null);
	let showDiff = $state(false);
	let commits = $state<GitCommit[]>([]);

	onMount(() => {
		void loadCommits();
	});

	$effect(() => {
		void loadCommits();
	});

	async function loadCommits() {
		try {
			commits = await backend.git.getLog(projectPath, 20);
		} catch {
			commits = [];
		}
	}

	async function handleFileClick(change: GitChange) {
		if (selectedFile === change.path && showDiff) {
			showDiff = false;
			selectedFile = null;
			return;
		}

		selectedFile = change.path;
		try {
			diffContent = await backend.git.getFileDiff(projectPath, change.path, change.staged);
			showDiff = true;
		} catch {
			diffContent = null;
			showDiff = false;
		}
	}

	function statusLabel(status: string): string {
		switch (status) {
			case 'M':
				return 'M';
			case 'A':
				return 'A';
			case 'D':
				return 'D';
			case 'R':
				return 'R';
			case '?':
				return '?';
			default:
				return status.charAt(0);
		}
	}

	function statusColor(status: string): string {
		if (status.startsWith('M')) return '#d29922';
		if (status.startsWith('A')) return '#3fb950';
		if (status.startsWith('D')) return '#f85149';
		if (status === '?') return '#8b949e';
		return '#8b949e';
	}

	function diffLineClass(line: string): string {
		if (line.startsWith('+++') || line.startsWith('---')) return 'meta';
		if (line.startsWith('@@')) return 'hunk';
		if (line.startsWith('+')) return 'added';
		if (line.startsWith('-')) return 'removed';
		return 'context';
	}

	let stagedFiles = $derived(summary.changes.filter((change) => change.staged));
	let unstagedFiles = $derived(summary.changes.filter((change) => !change.staged && change.status !== '?'));
	let untrackedFiles = $derived(summary.changes.filter((change) => change.status === '?'));
	let diffLines = $derived((diffContent ?? '').split('\n'));
</script>

<div class="git-panel">
	<div class="panel-header">
		<div>
			<span class="panel-title">Git</span>
			<p class="panel-subtitle">
				{summary.branch ?? 'Detached HEAD'}
				{#if summary.base_ref}
					· base {summary.base_ref}
				{/if}
			</p>
		</div>
		<button
			class="refresh-btn"
			onclick={() => {
				onRefresh?.();
				void loadCommits();
			}}
			title="Refresh"
		>
			↻
		</button>
	</div>

	{#if summary.changes.length === 0}
		<div class="empty-state">No changes.</div>
	{/if}

	{#if stagedFiles.length > 0}
		<div class="file-group">
			<div class="group-label">Staged ({stagedFiles.length})</div>
			{#each stagedFiles as change (change.path + '-staged')}
				<button
					class="file-item"
					class:selected={selectedFile === change.path}
					onclick={() => handleFileClick(change)}
				>
					<span class="file-status" style="color: {statusColor(change.status)}">{statusLabel(change.status)}</span>
					<span class="file-name">{change.path}</span>
					{#if change.additions != null || change.deletions != null}
						<span class="file-stats">
							{#if change.additions}<span class="adds">+{change.additions}</span>{/if}
							{#if change.deletions}<span class="dels">-{change.deletions}</span>{/if}
						</span>
					{/if}
				</button>
			{/each}
		</div>
	{/if}

	{#if unstagedFiles.length > 0}
		<div class="file-group">
			<div class="group-label">Modified ({unstagedFiles.length})</div>
			{#each unstagedFiles as change (change.path + '-unstaged')}
				<button
					class="file-item"
					class:selected={selectedFile === change.path}
					onclick={() => handleFileClick(change)}
				>
					<span class="file-status" style="color: {statusColor(change.status)}">{statusLabel(change.status)}</span>
					<span class="file-name">{change.path}</span>
					{#if change.additions != null || change.deletions != null}
						<span class="file-stats">
							{#if change.additions}<span class="adds">+{change.additions}</span>{/if}
							{#if change.deletions}<span class="dels">-{change.deletions}</span>{/if}
						</span>
					{/if}
				</button>
			{/each}
		</div>
	{/if}

	{#if untrackedFiles.length > 0}
		<div class="file-group">
			<div class="group-label">Untracked ({untrackedFiles.length})</div>
			{#each untrackedFiles as change (change.path + '-untracked')}
				<button class="file-item" onclick={() => handleFileClick(change)}>
					<span class="file-status" style="color: {statusColor(change.status)}">?</span>
					<span class="file-name">{change.path}</span>
				</button>
			{/each}
		</div>
	{/if}

	{#if showDiff && diffContent}
		<div class="diff-view">
			<div class="diff-header">
				<span>{selectedFile}</span>
				<button class="close-diff" onclick={() => (showDiff = false)}>×</button>
			</div>
			<pre class="diff-content">{#each diffLines as line}<span class={diffLineClass(line)}>{line}</span>
{/each}</pre>
		</div>
	{/if}

	<div class="commits-section">
		<div class="group-label">Commits</div>
		{#if commits.length === 0}
			<div class="empty-state compact">No recent commits.</div>
		{:else}
			{#each commits as commit (commit.hash)}
				<div class="commit-item">
					<div class="commit-topline">
						<span class="commit-hash">{commit.short_hash}</span>
						<span class="commit-date">{commit.date}</span>
					</div>
					<div class="commit-message">{commit.message}</div>
					<div class="commit-author">{commit.author}</div>
				</div>
			{/each}
		{/if}
	</div>
</div>

<style>
	.git-panel {
		height: 100%;
		background: #0d1117;
		font-size: 0.78rem;
		display: flex;
		flex-direction: column;
	}

	.panel-header {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		padding: 10px;
		border-bottom: 1px solid #30363d;
		background: #161b22;
	}

	.panel-title {
		font-weight: 600;
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: #8b949e;
	}

	.panel-subtitle {
		margin: 4px 0 0;
		color: #6e7681;
		font-size: 0.72rem;
	}

	.refresh-btn,
	.close-diff {
		background: none;
		border: none;
		color: #8b949e;
		cursor: pointer;
		font-size: 0.9rem;
		padding: 0 4px;
	}

	.refresh-btn:hover,
	.close-diff:hover {
		color: #f0f6fc;
	}

	.empty-state {
		padding: 14px 10px;
		color: #6e7681;
	}

	.empty-state.compact {
		padding-top: 6px;
	}

	.file-group,
	.commits-section {
		padding: 4px 0;
	}

	.group-label {
		padding: 4px 10px;
		font-size: 0.68rem;
		color: #8b949e;
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}

	.file-item {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		padding: 3px 10px;
		border: none;
		background: none;
		color: #c9d1d9;
		cursor: pointer;
		text-align: left;
		font-size: 0.75rem;
		font-family: 'JetBrains Mono', 'Fira Code', monospace;
	}

	.file-item:hover {
		background: #161b22;
	}

	.file-item.selected {
		background: #1f6feb22;
	}

	.file-status {
		font-weight: 700;
		width: 14px;
		text-align: center;
		flex-shrink: 0;
	}

	.file-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.file-stats {
		flex-shrink: 0;
		display: flex;
		gap: 4px;
	}

	.adds {
		color: #3fb950;
	}

	.dels {
		color: #f85149;
	}

	.diff-view {
		border-top: 1px solid #30363d;
	}

	.diff-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 4px 10px;
		background: #161b22;
		font-size: 0.72rem;
		color: #8b949e;
	}

	.diff-content {
		padding: 6px 10px;
		font-size: 0.7rem;
		font-family: 'JetBrains Mono', 'Fira Code', monospace;
		overflow: auto;
		max-height: 260px;
		white-space: pre;
		margin: 0;
		display: flex;
		flex-direction: column;
	}

	.diff-content span {
		display: block;
	}

	.diff-content .meta,
	.commit-author,
	.commit-date {
		color: #8b949e;
	}

	.diff-content .hunk {
		color: #58a6ff;
	}

	.diff-content .added {
		color: #3fb950;
	}

	.diff-content .removed {
		color: #f85149;
	}

	.diff-content .context,
	.commit-message {
		color: #c9d1d9;
	}

	.commits-section {
		margin-top: auto;
		border-top: 1px solid #30363d;
	}

	.commit-item {
		padding: 8px 10px;
		border-top: 1px solid rgba(48, 54, 61, 0.45);
	}

	.commit-topline {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		margin-bottom: 4px;
	}

	.commit-hash {
		font-family: 'JetBrains Mono', 'Fira Code', monospace;
		color: #58a6ff;
		font-size: 0.72rem;
	}

	.commit-message {
		font-size: 0.76rem;
	}

	.commit-author,
	.commit-date {
		font-size: 0.68rem;
	}
</style>
