<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import type { GitChange, GitSummary, DiffContext } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';
	import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree';
	import { TreeNode } from '$lib/components/ui/file-tree';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import Folder from '@lucide/svelte/icons/folder';

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
	let pendingPath = $state<string | null>(null);

	$effect(() => {
		projectPath;
		collapsedDirs.clear();
		pendingPath = null;
	});

	async function handleFileClick(change: GitChange) {
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

	function statusColorClass(status: string): string {
		switch (status) {
			case 'M': return 'text-warning';
			case 'A': return 'text-success';
			case 'D': return 'text-danger';
			case 'R': return 'text-accent';
			default: return 'text-muted';
		}
	}

	let stagedFiles = $derived(summary.changes.filter((c) => c.staged));
	let unstagedFiles = $derived(summary.changes.filter((c) => !c.staged && c.status !== '?'));

	let stagedTree = $derived(buildFileTree(stagedFiles));
	let unstagedTree = $derived(buildFileTree(unstagedFiles));
</script>

<div class="text-[0.78rem]">
	{#if stagedTree.length === 0 && unstagedTree.length === 0}
		<div class="py-2 px-2.5 text-subtle text-[0.75rem]">No changes.</div>
	{/if}

	{#snippet statusBadge(change: GitChange)}
		<span class="font-bold w-3.5 text-center shrink-0 {statusColorClass(change.status)}">{change.status}</span>
	{/snippet}

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
								<FolderOpen size={12} />
							{:else}
								<Folder size={12} />
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
				{#if node.change.status !== 'D' && (node.change.additions != null || node.change.deletions != null)}
					{@const net = (node.change.additions ?? 0) - (node.change.deletions ?? 0)}
					{#if net !== 0}
						<span class="shrink-0 {net > 0 ? 'text-success' : 'text-danger'}">
							{net > 0 ? '+' : ''}{net}
						</span>
					{:else}
						{@render statusBadge(node.change)}
					{/if}
				{:else}
					{@render statusBadge(node.change)}
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
</div>
