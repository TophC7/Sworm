<script lang="ts">
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

	$effect(() => {
		projectPath;
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

	function statusColorClass(status: string): string {
		if (status.startsWith('M')) return 'text-warning';
		if (status.startsWith('A')) return 'text-success';
		if (status.startsWith('D')) return 'text-danger';
		return 'text-muted';
	}

	function diffLineColorClass(line: string): string {
		if (line.startsWith('+++') || line.startsWith('---')) return 'text-muted';
		if (line.startsWith('@@')) return 'text-accent';
		if (line.startsWith('+')) return 'text-success';
		if (line.startsWith('-')) return 'text-danger';
		return 'text-fg';
	}

	let stagedFiles = $derived(summary.changes.filter((change) => change.staged));
	let unstagedFiles = $derived(summary.changes.filter((change) => !change.staged && change.status !== '?'));
	let untrackedFiles = $derived(summary.changes.filter((change) => change.status === '?'));
	let diffLines = $derived((diffContent ?? '').split('\n'));
</script>

<div class="h-full bg-ground text-[0.78rem] flex flex-col">
	<div class="flex items-start justify-between p-2.5 border-b border-edge bg-surface">
		<div>
			<span class="font-semibold text-[0.75rem] uppercase tracking-wide text-muted">Git</span>
			<p class="mt-1 mb-0 text-subtle text-[0.72rem]">
				{summary.branch ?? 'Detached HEAD'}
				{#if summary.base_ref}
					&middot; base {summary.base_ref}
				{/if}
			</p>
		</div>
		<button
			class="btn-ghost"
			onclick={() => {
				onRefresh?.();
				void loadCommits();
			}}
			title="Refresh"
		>
			&#8635;
		</button>
	</div>

	{#if summary.changes.length === 0}
		<div class="py-3.5 px-2.5 text-subtle">No changes.</div>
	{/if}

	{#snippet fileGroup(label: string, files: GitChange[], keySuffix: string)}
		{#if files.length > 0}
			<div class="py-1">
				<div class="px-2.5 py-1 text-[0.68rem] text-muted font-medium uppercase tracking-wide">
					{label} ({files.length})
				</div>
				{#each files as change (change.path + '-' + keySuffix)}
					<button
						class="flex items-center gap-1.5 w-full px-2.5 py-0.5 border-none bg-transparent text-fg cursor-pointer text-left text-[0.75rem] font-mono hover:bg-surface {selectedFile === change.path ? 'bg-accent-bg' : ''}"
						onclick={() => handleFileClick(change)}
					>
						<span class="font-bold w-3.5 text-center shrink-0 {statusColorClass(change.status)}">{statusLabel(change.status)}</span>
						<span class="flex-1 min-w-0 truncate">{change.path}</span>
						{#if change.additions != null || change.deletions != null}
							<span class="shrink-0 flex gap-1">
								{#if change.additions}<span class="text-success">+{change.additions}</span>{/if}
								{#if change.deletions}<span class="text-danger">-{change.deletions}</span>{/if}
							</span>
						{/if}
					</button>
				{/each}
			</div>
		{/if}
	{/snippet}

	{@render fileGroup('Staged', stagedFiles, 'staged')}
	{@render fileGroup('Modified', unstagedFiles, 'unstaged')}
	{@render fileGroup('Untracked', untrackedFiles, 'untracked')}

	{#if showDiff && diffContent}
		<div class="border-t border-edge">
			<div class="flex items-center justify-between px-2.5 py-1 bg-surface text-[0.72rem] text-muted">
				<span>{selectedFile}</span>
				<button
					class="btn-ghost"
					onclick={() => (showDiff = false)}
				>&times;</button>
			</div>
			<pre class="px-2.5 py-1.5 text-[0.7rem] font-mono overflow-auto max-h-[260px] whitespace-pre m-0 flex flex-col">{#each diffLines as line}<span class="block {diffLineColorClass(line)}">{line}</span>
{/each}</pre>
		</div>
	{/if}

	<div class="mt-auto border-t border-edge py-1">
		<div class="px-2.5 py-1 text-[0.68rem] text-muted font-medium uppercase tracking-wide">
			Commits
		</div>
		{#if commits.length === 0}
			<div class="pt-1.5 px-2.5 text-subtle">No recent commits.</div>
		{:else}
			{#each commits as commit (commit.hash)}
				<div class="px-2.5 py-2 border-t border-edge/45">
					<div class="flex items-center justify-between gap-2.5 mb-1">
						<span class="font-mono text-accent text-[0.72rem]">{commit.short_hash}</span>
						<span class="text-muted text-[0.68rem]">{commit.date}</span>
					</div>
					<div class="text-fg text-[0.76rem]">{commit.message}</div>
					<div class="text-muted text-[0.68rem]">{commit.author}</div>
				</div>
			{/each}
		{/if}
	</div>
</div>
