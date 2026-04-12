<script lang="ts">
	import GitCommitLog from '$lib/components/GitCommitLog.svelte'
	import GitFileTree from '$lib/components/GitFileTree.svelte'
	import { Badge } from '$lib/components/ui/badge'
	import { Button } from '$lib/components/ui/button'
	import {
	  ResizableHandle,
	  ResizablePane,
	  ResizablePaneGroup
	} from '$lib/components/ui/resizable'
	import {
	  Sidebar,
	  SidebarContent,
	  SidebarHeader,
	  SidebarProvider,
	  SidebarRail,
	  SidebarTrigger
	} from '$lib/components/ui/sidebar'
	import {
	  isGitSidebarCollapsed,
	  setGitSidebarCollapsed
	} from '$lib/stores/ui.svelte'
	import type { DiffContext, GitSummary } from '$lib/types/backend'
	import { PanelLeftClose } from '@lucide/svelte'
	import ArrowDown from '@lucide/svelte/icons/arrow-down'
	import ArrowUp from '@lucide/svelte/icons/arrow-up'
	import GitBranchIcon from '@lucide/svelte/icons/git-branch'
	import RotateCw from '@lucide/svelte/icons/rotate-cw'

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
			<div class="flex items-center gap-1.5">
				<span class="font-semibold text-[0.7rem] uppercase tracking-wide text-muted">Git</span>
				{#if onRefresh}
					<Button variant="ghost" size="icon-sm" onclick={onRefresh} title="Refresh">
						<RotateCw size={11} />
					</Button>
				{/if}
			</div>
			<div class="flex items-center gap-1">
				<SidebarTrigger>
					<PanelLeftClose size={12} />
				</SidebarTrigger>
			</div>
		</SidebarHeader>

		{#if !collapsed && summary?.branch}
			<div class="flex items-center gap-1.5 px-2.5 py-1 border-b border-edge shrink-0">
				<Badge variant="default" class="gap-1 font-mono text-[0.68rem] normal-case">
					<GitBranchIcon size={10} />
					{summary.branch}
				</Badge>
				{#if (summary.ahead ?? 0) > 0}
					<Badge variant="success" class="gap-0.5 text-[0.68rem]">
						<ArrowUp size={9} />{summary.ahead}
					</Badge>
				{/if}
				{#if (summary.behind ?? 0) > 0}
					<Badge variant="danger" class="gap-0.5 text-[0.68rem]">
						<ArrowDown size={9} />{summary.behind}
					</Badge>
				{/if}
			</div>
		{/if}

		<SidebarContent>
			{#if summary}
				<ResizablePaneGroup direction="vertical">
					<ResizablePane defaultSize={60} minSize={15}>
						<div class="overflow-y-auto h-full">
							<GitFileTree
								{summary}
								{projectPath}
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
