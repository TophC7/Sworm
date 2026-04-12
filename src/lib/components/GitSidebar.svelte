<script lang="ts">
	import type { GitSummary, DiffContext } from '$lib/types/backend';
	import GitFileTree from '$lib/components/GitFileTree.svelte';
	import GitCommitLog from '$lib/components/GitCommitLog.svelte';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import ChevronsLeft from '@lucide/svelte/icons/chevrons-left';
	import {
		isGitSidebarCollapsed,
		setGitSidebarCollapsed
	} from '$lib/stores/ui.svelte';
	import {
		SidebarProvider,
		Sidebar,
		SidebarHeader,
		SidebarContent,
		SidebarRail,
		SidebarTrigger
	} from '$lib/components/ui/sidebar';
	import {
		ResizablePaneGroup,
		ResizablePane,
		ResizableHandle
	} from '$lib/components/ui/resizable';

	let {
		summary,
		projectPath,
		onRefresh,
		onViewDiff,
		activeDiffFile,
		onDiffError,
		onPersistDiff
	}: {
		summary: GitSummary | null;
		projectPath: string;
		onRefresh?: () => void;
		onViewDiff?: (filePath: string, context: DiffContext | null) => void;
		activeDiffFile?: string | null;
		onDiffError?: (message: string | null) => void;
		onPersistDiff?: () => void;
	} = $props();

	let collapsed = $derived(isGitSidebarCollapsed());
</script>

<SidebarProvider open={!collapsed} onOpenChange={(open) => setGitSidebarCollapsed(!open)}>
	<Sidebar>
		<SidebarRail>
			<GitBranchIcon size={16} class="text-muted" />
		</SidebarRail>

		<SidebarHeader>
			<span class="font-semibold text-[0.7rem] uppercase tracking-wide text-muted">Git</span>
			<div class="flex items-center gap-1">
				{#if summary?.branch}
					<span class="text-[0.68rem] text-muted">{summary.branch}</span>
				{/if}
				<SidebarTrigger>
					<ChevronsLeft size={12} />
				</SidebarTrigger>
			</div>
		</SidebarHeader>

		<SidebarContent>
			{#if summary}
				<ResizablePaneGroup direction="vertical">
					<ResizablePane defaultSize={60} minSize={15}>
						<div class="overflow-y-auto h-full">
							<GitFileTree
								{summary}
								{projectPath}
								{onRefresh}
								{onViewDiff}
								{activeDiffFile}
								{onDiffError}
								{onPersistDiff}
							/>
						</div>
					</ResizablePane>
					<ResizableHandle />
					<ResizablePane defaultSize={40} minSize={15}>
						<div class="overflow-y-auto h-full">
							<GitCommitLog {projectPath} />
						</div>
					</ResizablePane>
				</ResizablePaneGroup>
			{:else}
				<div class="py-3 px-2.5 text-subtle text-[0.75rem]">Loading git info&hellip;</div>
			{/if}
		</SidebarContent>
	</Sidebar>
</SidebarProvider>
