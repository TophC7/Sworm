<script lang="ts">
	import type { Tab, PaneState } from '$lib/stores/workspace.svelte';
	import {
		setFocusedPane,
		getFocusedPaneSlot,
		getDraggedTab,
		canSplitPane,
		moveTabToPane,
		splitPaneAt,
		setActiveTab,
		endTabDrag
	} from '$lib/stores/workspace.svelte';
	import PaneTabBar from '$lib/components/PaneTabBar.svelte';
	import SessionTerminal from '$lib/components/SessionTerminal.svelte';
	import DiffViewer from '$lib/components/DiffViewer.svelte';
	import NewSessionView from '$lib/components/NewSessionView.svelte';
	import { getSessions, updateSessionInList } from '$lib/stores/sessions.svelte';
	import { refreshGit } from '$lib/stores/git.svelte';

	type DropIntent = 'move' | 'right' | 'down' | null;

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

	let isFocused = $derived(getFocusedPaneSlot() === pane.slot);
	let draggedTab = $derived(getDraggedTab());
	let canSplitRight = $derived(canSplitPane(projectId, pane.slot, 'right'));
	let canSplitDown = $derived(canSplitPane(projectId, pane.slot, 'down'));

	let sessions = $derived(getSessions());
	let activeTab = $derived(
		pane.activeTabId ? tabs.find((tab) => tab.id === pane.activeTabId) ?? null : null
	);
	let paneSession = $derived(
		activeTab?.kind === 'session'
			? sessions.find((session) => session.id === activeTab.sessionId) ?? null
			: null
	);

	let showNewSession = $state(false);
	let dropIntent = $state<DropIntent>(null);
	let dropActive = $state(false);

	function handleFocus() {
		setFocusedPane(pane.slot);
	}

	function handleNewSession() {
		handleFocus();
		showNewSession = true;
	}

	function dragTabId(): string | null {
		if (!draggedTab || draggedTab.projectId !== projectId) return null;
		return draggedTab.tabId;
	}

	function computeDropIntent(event: DragEvent): DropIntent {
		if (!dragTabId()) return null;

		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
		const x = rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0;
		const y = rect.height > 0 ? (event.clientY - rect.top) / rect.height : 0;

		if (canSplitRight && x >= 0.72) return 'right';
		if (canSplitDown && y >= 0.72) return 'down';
		return 'move';
	}

	function handleDragOver(event: DragEvent) {
		if (!dragTabId()) return;

		event.preventDefault();
		handleFocus();
		dropActive = true;
		dropIntent = computeDropIntent(event);

		if (event.dataTransfer) {
			event.dataTransfer.dropEffect = 'move';
		}
	}

	function handleDragLeave(event: DragEvent) {
		const next = event.relatedTarget as Node | null;
		if (next && (event.currentTarget as HTMLElement).contains(next)) return;
		dropActive = false;
		dropIntent = null;
	}

	function handleDrop(event: DragEvent) {
		const tabId = dragTabId();
		if (!tabId) return;

		event.preventDefault();
		handleFocus();
		showNewSession = false;

		const intent = computeDropIntent(event);
		if (intent === 'move') {
			moveTabToPane(projectId, tabId, pane.slot);
			setActiveTab(projectId, pane.slot, tabId);
		} else if (intent) {
			const newSlot = splitPaneAt(projectId, pane.slot, intent);
			if (newSlot) {
				moveTabToPane(projectId, tabId, newSlot);
				setActiveTab(projectId, newSlot, tabId);
			}
		}

		dropActive = false;
		dropIntent = null;
		endTabDrag();
	}

	$effect(() => {
		if (!draggedTab) {
			dropActive = false;
			dropIntent = null;
		}
	});
</script>

<div
	class="relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden {isFocused ? 'ring-1 ring-accent/30' : ''}"
	onfocusin={handleFocus}
	onpointerdown={handleFocus}
	ondragover={handleDragOver}
	ondragleave={handleDragLeave}
	ondrop={handleDrop}
	role="region"
>
	<PaneTabBar
		{tabs}
		activeTabId={pane.activeTabId}
		paneSlot={pane.slot}
		{projectId}
		{isFocused}
		onNewSession={handleNewSession}
		onTabSelected={() => (showNewSession = false)}
	/>

	<div class="flex-1 flex flex-col min-h-0 overflow-hidden">
		{#if showNewSession || !activeTab}
			<NewSessionView onCreated={() => (showNewSession = false)} />
		{:else}
			{#if activeTab.kind === 'session' && paneSession}
				<SessionTerminal
					session={paneSession}
					onStatusChange={(status) => {
						updateSessionInList(paneSession.id, { status });
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
				/>
			{/if}
		{/if}
	</div>

	{#if draggedTab?.projectId === projectId && dropActive}
		<div class="pointer-events-none absolute inset-0 z-20 p-2">
			<div class="absolute inset-2 rounded-xl bg-ground/55 border border-edge/70"></div>
			<div
				class="absolute inset-4 rounded-lg border flex items-center justify-center text-[0.72rem] uppercase tracking-wide
					{dropIntent === 'move'
						? 'border-accent bg-accent/12 text-accent'
						: 'border-edge/70 bg-surface/45 text-muted'}"
			>
				Move Here
			</div>

			{#if canSplitRight}
				<div
					class="absolute top-4 bottom-4 right-4 w-[28%] rounded-lg border flex items-center justify-center text-[0.72rem] uppercase tracking-wide
						{dropIntent === 'right'
							? 'border-accent bg-accent/14 text-accent'
							: 'border-edge/70 bg-surface/55 text-muted'}"
				>
					Split Right
				</div>
			{/if}

			{#if canSplitDown}
				<div
					class="absolute left-4 right-4 bottom-4 h-[28%] rounded-lg border flex items-center justify-center text-[0.72rem] uppercase tracking-wide
						{dropIntent === 'down'
							? 'border-accent bg-accent/14 text-accent'
							: 'border-edge/70 bg-surface/55 text-muted'}"
				>
					Split Down
				</div>
			{/if}
		</div>
	{/if}
</div>
