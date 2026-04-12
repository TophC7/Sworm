<script lang="ts">
	import type { GitCommit } from '$lib/types/backend';
	import { backend } from '$lib/api/backend';

	let { projectPath }: { projectPath: string } = $props();

	let commits = $state<GitCommit[]>([]);
	let currentPath = '';

	$effect(() => {
		const path = projectPath;
		currentPath = path;
		void loadCommits(path);
	});

	async function loadCommits(path: string) {
		try {
			const result = await backend.git.getLog(path, 20);
			// Guard against stale results from rapid project switching
			if (path === currentPath) commits = result;
		} catch {
			if (path === currentPath) commits = [];
		}
	}
</script>

<div class="text-[0.78rem]">
	<div class="flex items-center justify-between px-2.5 py-1.5">
		<span class="font-semibold text-[0.7rem] uppercase tracking-wide text-muted">Commits</span>
	</div>

	{#if commits.length === 0}
		<div class="py-2 px-2.5 text-subtle text-[0.75rem]">No recent commits.</div>
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
