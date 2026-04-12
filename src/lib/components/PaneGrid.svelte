<script lang="ts">
	import type { Tab, PaneState, PaneSlot } from '$lib/stores/workspace.svelte';
	import { getPanes, getSplitMode, getAllTabs } from '$lib/stores/workspace.svelte';
	import { ResizablePaneGroup, ResizablePane, ResizableHandle } from '$lib/components/ui/resizable';
	import Pane from '$lib/components/Pane.svelte';

	let {
		projectId,
		projectPath
	}: {
		projectId: string;
		projectPath: string;
	} = $props();

	let panes = $derived(getPanes(projectId));
	let splitMode = $derived(getSplitMode(projectId));
	let allTabs = $derived(getAllTabs(projectId));

	// Single-pass slot lookup instead of 6 separate .find() scans
	let panesBySlot = $derived(
		Object.fromEntries(panes.map((p) => [p.slot, p])) as Partial<Record<PaneSlot, PaneState>>
	);
	let leftPane = $derived(panesBySlot['left']);
	let rightPane = $derived(panesBySlot['right']);
	let topLeft = $derived(panesBySlot['top-left']);
	let topRight = $derived(panesBySlot['top-right']);
	let bottomLeft = $derived(panesBySlot['bottom-left']);
	let bottomRight = $derived(panesBySlot['bottom-right']);

	function tabsForPane(pane: PaneState): Tab[] {
		return pane.tabs
			.map((id) => allTabs.find((t) => t.id === id))
			.filter((t): t is Tab => t !== undefined);
	}
</script>

<div class="flex-1 flex flex-col min-h-0 overflow-hidden">
	{#if splitMode === 'single'}
		{#if panes[0]}
			<Pane pane={panes[0]} tabs={tabsForPane(panes[0])} {projectId} {projectPath} />
		{/if}

	{:else if splitMode === 'horizontal'}
		<ResizablePaneGroup direction="horizontal" autoSaveId="{projectId}-h">
			{#if leftPane}
				<ResizablePane defaultSize={50} minSize={20}>
					<Pane pane={leftPane} tabs={tabsForPane(leftPane)} {projectId} {projectPath} />
				</ResizablePane>
			{/if}
			<ResizableHandle />
			{#if rightPane}
				<ResizablePane defaultSize={50} minSize={20}>
					<Pane pane={rightPane} tabs={tabsForPane(rightPane)} {projectId} {projectPath} />
				</ResizablePane>
			{/if}
		</ResizablePaneGroup>

	{:else if splitMode === 'vertical'}
		<ResizablePaneGroup direction="vertical" autoSaveId="{projectId}-v">
			{#if leftPane}
				<ResizablePane defaultSize={50} minSize={20}>
					<Pane pane={leftPane} tabs={tabsForPane(leftPane)} {projectId} {projectPath} />
				</ResizablePane>
			{/if}
			<ResizableHandle />
			{#if rightPane}
				<ResizablePane defaultSize={50} minSize={20}>
					<Pane pane={rightPane} tabs={tabsForPane(rightPane)} {projectId} {projectPath} />
				</ResizablePane>
			{/if}
		</ResizablePaneGroup>

	{:else if splitMode === 'quad'}
		<ResizablePaneGroup direction="vertical" autoSaveId="{projectId}-quad-v">
			<ResizablePane defaultSize={50} minSize={20}>
				<ResizablePaneGroup direction="horizontal" autoSaveId="{projectId}-quad-ht">
					{#if topLeft}
						<ResizablePane defaultSize={50} minSize={20}>
							<Pane pane={topLeft} tabs={tabsForPane(topLeft)} {projectId} {projectPath} />
						</ResizablePane>
					{/if}
					<ResizableHandle />
					{#if topRight}
						<ResizablePane defaultSize={50} minSize={20}>
							<Pane pane={topRight} tabs={tabsForPane(topRight)} {projectId} {projectPath} />
						</ResizablePane>
					{/if}
				</ResizablePaneGroup>
			</ResizablePane>
			<ResizableHandle />
			<ResizablePane defaultSize={50} minSize={20}>
				<ResizablePaneGroup direction="horizontal" autoSaveId="{projectId}-quad-hb">
					{#if bottomLeft}
						<ResizablePane defaultSize={50} minSize={20}>
							<Pane pane={bottomLeft} tabs={tabsForPane(bottomLeft)} {projectId} {projectPath} />
						</ResizablePane>
					{/if}
					<ResizableHandle />
					{#if bottomRight}
						<ResizablePane defaultSize={50} minSize={20}>
							<Pane pane={bottomRight} tabs={tabsForPane(bottomRight)} {projectId} {projectPath} />
						</ResizablePane>
					{/if}
				</ResizablePaneGroup>
			</ResizablePane>
		</ResizablePaneGroup>
	{/if}
</div>
