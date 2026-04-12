<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import type { GitChange, GitSummary, DiffContext } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';
	import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree';
	import { TreeNode } from '$lib/components/ui/file-tree';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	let {
		summary,
		projectPath,
		onViewDiff,
		activeDiffFile,
		onDiffError,
		onPersistDiff
	}: {
		summary: GitSummary;
		projectPath: string;
		onViewDiff?: (filePath: string, context: DiffContext | null) => void;
		activeDiffFile?: string | null;
		onDiffError?: (message: string | null) => void;
		onPersistDiff?: () => void;
	} = $props();

	let collapsedDirs = new SvelteSet<string>();
	// Track which file we're currently loading/viewing to debounce rapid clicks
	let pendingPath = $state<string | null>(null);

	$effect(() => {
		projectPath;
		collapsedDirs.clear();
		pendingPath = null;
	});

	async function handleFileClick(change: GitChange) {
		// Already viewing or loading this file — no-op
		if (activeDiffFile === change.path || pendingPath === change.path) return;

		pendingPath = change.path;
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
		} finally {
			pendingPath = null;
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

	let stagedFiles = $derived(summary.changes.filter((c) => c.staged));
	let unstagedFiles = $derived(summary.changes.filter((c) => !c.staged && c.status !== '?'));
	let untrackedFiles = $derived(summary.changes.filter((c) => c.status === '?'));

	let stagedTree = $derived(buildFileTree(stagedFiles));
	let unstagedTree = $derived(buildFileTree(unstagedFiles));
	let untrackedTree = $derived(buildFileTree(untrackedFiles));
</script>

<div class="text-[0.78rem]">
	{#if summary.changes.length === 0}
		<div class="py-2 px-2.5 text-subtle text-[0.75rem]">No changes.</div>
	{/if}

	{#snippet treeNode(section: string, node: FileTreeNode, depth: number)}
		{#if node.type === 'directory'}
			<TreeNode expanded={!collapsedDirs.has(getDirKey(section, node.path))} {depth}>
				{#snippet label()}
					<button
						class="flex items-center gap-1 w-full py-0.5 border-none bg-transparent text-muted cursor-pointer text-left text-[0.72rem] font-mono hover:bg-surface"
						style="padding-left: {depth * 12 + 10}px"
						onclick={() => toggleDir(section, node.path)}
					>
						<span class="w-3 flex items-center justify-center shrink-0">
							{#if !collapsedDirs.has(getDirKey(section, node.path))}
								<ChevronDown size={10} />
							{:else}
								<ChevronRight size={10} />
							{/if}
						</span>
						<span class="truncate">{node.name}</span>
					</button>
				{/snippet}
				{#each node.children as child (child.path)}
					{@render treeNode(section, child, depth + 1)}
				{/each}
			</TreeNode>
		{:else if node.change}
			<button
				class="flex items-center gap-1.5 w-full py-0.5 border-none bg-transparent text-fg cursor-pointer text-left text-[0.75rem] font-mono hover:bg-surface {activeDiffFile === node.change.path ? 'bg-accent-bg' : ''}"
				style="padding-left: {depth * 12 + 10}px"
				onclick={() => handleFileClick(node.change!)}
				ondblclick={() => onPersistDiff?.()}
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
</div>
