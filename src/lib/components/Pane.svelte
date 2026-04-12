<script lang="ts">
	import type { Tab, TabId, PaneSlot, PaneState } from '$lib/stores/workspace.svelte';
	import {
		setFocusedPane,
		getFocusedPaneSlot
	} from '$lib/stores/workspace.svelte';
	import PaneTabBar from '$lib/components/PaneTabBar.svelte';
	import SessionTerminal from '$lib/components/SessionTerminal.svelte';
	import DiffViewer from '$lib/components/DiffViewer.svelte';
	import { getSessions, updateSessionInList } from '$lib/stores/sessions.svelte';
	import { closeTab, collapsePaneIfEmpty } from '$lib/stores/workspace.svelte';
	import { refreshGit } from '$lib/stores/git.svelte';

	let {
		pane,
		tabs,
		projectId,
		projectPath
	}: {
		pane: PaneState;
		tabs: Tab[];
		projectId: string;
		projectPath: string;
	} = $props();

	let focusedSlot = $derived(getFocusedPaneSlot());
	let isFocused = $derived(focusedSlot === pane.slot);

	let sessions = $derived(getSessions());
	let activeTab = $derived(
		pane.activeTabId ? tabs.find((t) => t.id === pane.activeTabId) ?? null : null
	);
	// Look up the session by ID from the list — not the global active singleton
	let paneSession = $derived(
		activeTab?.kind === 'session'
			? sessions.find((s) => s.id === activeTab.sessionId) ?? null
			: null
	);

	function handleFocus() {
		setFocusedPane(pane.slot);
	}

	function handleCloseTab() {
		if (activeTab) {
			closeTab(projectId, activeTab.id);
			collapsePaneIfEmpty(projectId);
		}
	}
</script>

<div
	class="flex flex-col flex-1 min-w-0 min-h-0 overflow-hidden relative {isFocused ? 'ring-1 ring-accent/30' : ''}"
	onfocusin={handleFocus}
	role="region"
>
	<PaneTabBar
		{tabs}
		activeTabId={pane.activeTabId}
		paneSlot={pane.slot}
		{projectId}
		{isFocused}
	/>

	<div class="flex-1 flex flex-col min-h-0 overflow-hidden">
		{#if activeTab}
			{#if activeTab.kind === 'session' && paneSession}
				<SessionTerminal
					session={paneSession}
					onStatusChange={(status) => {
						updateSessionInList(paneSession!.id, { status });
						if (status === 'exited' || status === 'stopped') {
							void refreshGit(projectId, projectPath);
						}
					}}
				/>
			{:else if activeTab.kind === 'diff'}
				<DiffViewer
					rawDiff={activeTab.context.raw_diff}
					filePath={activeTab.filePath}
					oldContent={activeTab.context.old_content}
					newContent={activeTab.context.new_content}
					onClose={handleCloseTab}
				/>
			{/if}
		{:else}
			<div class="flex-1 flex items-center justify-center text-subtle text-[0.82rem]">
				Empty pane
			</div>
		{/if}
	</div>
</div>
