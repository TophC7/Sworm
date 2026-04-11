<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import type { GitChange, GitCommit, GitSummary, DiffContext } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';
	import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree';

	let {
		summary,
		projectPath,
		onRefresh,
		onViewDiff,
		activeDiffFile,
		onDiffError
	}: {
		summary: GitSummary;
		projectPath: string;
		onRefresh?: () => void;
		onViewDiff?: (filePath: string, context: DiffContext | null) => void;
		activeDiffFile?: string | null;
		onDiffError?: (message: string | null) => void;
	} = $props();

	let commits = $state<GitCommit[]>([]);
	// Track manually collapsed dirs — all dirs are expanded by default
	let collapsedDirs = new SvelteSet<string>();

	$effect(() => {
		projectPath;
		void loadCommits();
	});

	$effect(() => {
		projectPath;
		collapsedDirs.clear();
	});

	async function loadCommits() {
		try {
			commits = await backend.git.getLog(projectPath, 20);
		} catch {
			commits = [];
		}
	}

	async function handleFileClick(change: GitChange) {
		// Toggle off if clicking same file
		if (activeDiffFile === change.path) {
			onDiffError?.(null);
			onViewDiff?.(change.path, null);
			return;
		}

		try {
			const ctx = await backend.git.getDiffContext(projectPath, change.path, change.staged);
			if (ctx?.raw_diff) {
				onDiffError?.(null);
				onViewDiff?.(change.path, ctx);
				return;
			}

			onDiffError?.(`No textual diff is available for ${change.path}.`);
		} catch {
			onDiffError?.(`Failed to load the diff for ${change.path}.`);
		}
	}

	function getDirKey(section: string, path: string): string {
		return `${section}:${path}`;
	}

	function toggleDir(section: string, path: string) {
		const key = getDirKey(section, path);
		if (collapsedDirs.has(key)) {
			collapsedDirs.delete(key);
		} else {
			collapsedDirs.add(key);
		}
	}

	function statusLabel(status: string): string {
		switch (status) {
			case 'M': return 'M';
			case 'A': return 'A';
			case 'D': return 'D';
			case 'R': return 'R';
			case '?': return '?';
			default: return status.charAt(0);
		}
	}

	function statusColorClass(status: string): string {
		if (status.startsWith('M')) return 'text-warning';
		if (status.startsWith('A')) return 'text-success';
		if (status.startsWith('D')) return 'text-danger';
		return 'text-muted';
	}

	let stagedFiles = $derived(summary.changes.filter((change) => change.staged));
	let unstagedFiles = $derived(summary.changes.filter((change) => !change.staged && change.status !== '?'));
	let untrackedFiles = $derived(summary.changes.filter((change) => change.status === '?'));

	let stagedTree = $derived(buildFileTree(stagedFiles));
	let unstagedTree = $derived(buildFileTree(unstagedFiles));
	let untrackedTree = $derived(buildFileTree(untrackedFiles));
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

	<!-- Recursive tree node renderer -->
	{#snippet treeNode(section: string, node: FileTreeNode, depth: number)}
		{#if node.type === 'directory'}
			<button
				class="flex items-center gap-1 w-full py-0.5 border-none bg-transparent text-muted cursor-pointer text-left text-[0.72rem] font-mono hover:bg-surface"
				style="padding-left: {depth * 12 + 10}px"
				onclick={() => toggleDir(section, node.path)}
			>
				<span class="w-3 text-center shrink-0 text-[0.6rem]">
					{!collapsedDirs.has(getDirKey(section, node.path)) ? '\u25BE' : '\u25B8'}
				</span>
				<span class="truncate">{node.name}</span>
			</button>
			{#if !collapsedDirs.has(getDirKey(section, node.path))}
				{#each node.children as child (child.path)}
					{@render treeNode(section, child, depth + 1)}
				{/each}
			{/if}
		{:else if node.change}
			<button
				class="flex items-center gap-1.5 w-full py-0.5 border-none bg-transparent text-fg cursor-pointer text-left text-[0.75rem] font-mono hover:bg-surface {activeDiffFile === node.change.path ? 'bg-accent-bg' : ''}"
				style="padding-left: {depth * 12 + 10}px"
				onclick={() => handleFileClick(node.change!)}
			>
				<span class="flex-1 min-w-0 truncate">{node.name}</span>
				<span class="font-bold w-3.5 text-center shrink-0 {statusColorClass(node.change.status)}">{statusLabel(node.change.status)}</span>
				{#if node.change.additions != null || node.change.deletions != null}
					<span class="shrink-0 flex gap-1 pr-2">
						{#if node.change.additions}<span class="text-success">+{node.change.additions}</span>{/if}
						{#if node.change.deletions}<span class="text-danger">-{node.change.deletions}</span>{/if}
					</span>
				{/if}
			</button>
		{/if}
	{/snippet}

	<!-- File group with tree rendering -->
	{#snippet fileGroup(label: string, tree: FileTreeNode[], keySuffix: string)}
		{#if tree.length > 0}
			<div class="py-1">
				<div class="px-2.5 py-1 text-[0.68rem] text-muted font-medium uppercase tracking-wide">
					{label} ({countFiles(tree)})
				</div>
				{#each tree as node (node.path + '-' + keySuffix)}
					{@render treeNode(keySuffix, node, 0)}
				{/each}
			</div>
		{/if}
	{/snippet}

	{@render fileGroup('Staged', stagedTree, 'staged')}
	{@render fileGroup('Modified', unstagedTree, 'unstaged')}
	{@render fileGroup('Untracked', untrackedTree, 'untracked')}

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
