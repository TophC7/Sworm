<script lang="ts">
	import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'
	import { getSessions, removeSession } from '$lib/stores/sessions.svelte'
	import type { PaneSlot, Tab, TabId } from '$lib/stores/workspace.svelte'
	import {
	  canSplitPane,
	  closeTab,
	  collapsePaneIfEmpty,
	  endTabDrag,
	  moveTabToPane,
	  promoteTemporaryTab,
	  setActiveTab,
	  splitPaneAt,
	  startTabDrag
	} from '$lib/stores/workspace.svelte'
	import { statusColorClass, statusIcon } from '$lib/utils/session'
	import Plus from '@lucide/svelte/icons/plus'

	let {
		tabs,
		activeTabId,
		paneSlot,
		projectId,
		isFocused = false,
		onNewSession,
		onTabSelected
	}: {
		tabs: Tab[];
		activeTabId: TabId | null;
		paneSlot: PaneSlot;
		projectId: string;
		isFocused?: boolean;
		onNewSession?: () => void;
		onTabSelected?: () => void;
	} = $props();

	let contextMenuOpen = $state(false);
	let contextMenuTabId = $state<TabId | null>(null);
	let contextMenuPos = $state({ x: 0, y: 0 });
	let canSplitRight = $derived(canSplitPane(projectId, paneSlot, 'right'));
	let canSplitDown = $derived(canSplitPane(projectId, paneSlot, 'down'));
	let sessions = $derived(getSessions());

	function handleTabClick(tabId: TabId) {
		setActiveTab(projectId, paneSlot, tabId);
		onTabSelected?.();
	}

	async function handleTabClose(e: Event, tabId: TabId) {
		e.stopPropagation();

		const tab = tabs.find((candidate) => candidate.id === tabId);
		if (!tab) return;

		if (tab.kind === 'session') {
			await removeSession(tab.sessionId, projectId);
			return;
		}

		closeTab(projectId, tabId);
		collapsePaneIfEmpty(projectId);
	}

	function handleContextMenu(e: MouseEvent, tabId: TabId) {
		e.preventDefault();
		contextMenuTabId = tabId;
		contextMenuPos = { x: e.clientX, y: e.clientY };
		contextMenuOpen = true;
	}

	function closeContextMenu() {
		contextMenuOpen = false;
		contextMenuTabId = null;
	}

	function handleSplit(direction: 'right' | 'down') {
		if (!contextMenuTabId) return;

		const newSlot = splitPaneAt(projectId, paneSlot, direction);
		if (newSlot) {
			moveTabToPane(projectId, contextMenuTabId, newSlot);
		}

		closeContextMenu();
	}

	function tabLabel(tab: Tab): string {
		if (tab.kind === 'session') return tab.title;
		return tab.filePath.split('/').pop() ?? tab.filePath;
	}

	function sessionStatus(tab: Tab): string | null {
		if (tab.kind !== 'session') return null;
		return sessions.find((session) => session.id === tab.sessionId)?.status ?? null;
	}

	function handleDragStart(e: DragEvent, tabId: TabId) {
		startTabDrag(projectId, tabId);
		e.dataTransfer?.setData('text/plain', tabId);
		if (e.dataTransfer) {
			e.dataTransfer.effectAllowed = 'move';
		}
	}

	function handleDragEnd() {
		endTabDrag();
	}

	function handleAuxClick(e: MouseEvent, tabId: TabId) {
		if (e.button !== 1) return;
		void handleTabClose(e, tabId);
	}

	$effect(() => {
		if (!contextMenuOpen) return;
		const close = () => closeContextMenu();
		const onKeydown = (e: KeyboardEvent) => { if (e.key === 'Escape') close(); };
		window.addEventListener('blur', close);
		window.addEventListener('scroll', close, true);
		window.addEventListener('keydown', onKeydown);
		return () => {
			window.removeEventListener('blur', close);
			window.removeEventListener('scroll', close, true);
			window.removeEventListener('keydown', onKeydown);
		};
	});
</script>

<TabStrip
	variant="pane"
	ariaLabel="Pane tabs"
	class={isFocused ? 'border-edge' : 'border-edge/50'}
>
	{#each tabs as tab (tab.id)}
		<TabButton
			variant="pane"
			active={activeTabId === tab.id}
			draggable={true}
			onclick={() => handleTabClick(tab.id)}
			ondblclick={() => {
				if (tab.kind === 'diff' && tab.temporary) {
					promoteTemporaryTab(tab.id);
				}
			}}
			oncontextmenu={(e) => handleContextMenu(e, tab.id)}
			onauxclick={(e) => handleAuxClick(e, tab.id)}
			ondragstart={(e) => handleDragStart(e, tab.id)}
			ondragend={handleDragEnd}
			onClose={(e) => handleTabClose(e, tab.id)}
		>
			{#if tab.kind === 'session'}
				{#snippet leading()}
					{@const status = sessionStatus(tab)}
					<span class="text-[0.55rem] shrink-0 {status ? statusColorClass(status) : 'text-muted'}">
						{status ? statusIcon(status) : '◌'}
					</span>
				{/snippet}
			{/if}
			<span class="truncate max-w-[120px] {tab.kind === 'diff' && tab.temporary ? 'italic' : ''}">
				{tabLabel(tab)}
			</span>
		</TabButton>
	{/each}

	{#snippet trailing()}
		<button
			class="flex items-center justify-center w-7 h-7 shrink-0 border-none text-muted cursor-pointer text-sm hover:text-bright transition-colors ml-0.5 sticky right-0 bg-ground"
			onclick={onNewSession}
			title="New session"
		>
			<Plus size={14} />
		</button>
	{/snippet}
</TabStrip>

{#if contextMenuOpen}
	<div class="fixed inset-0 z-40" onclick={closeContextMenu} role="none"></div>
	<div
		class="fixed z-50 min-w-[140px] bg-raised border border-edge rounded-lg shadow-[0_8px_24px_rgba(0,0,0,0.4)] py-1 text-[0.78rem]"
		style="left: {contextMenuPos.x}px; top: {contextMenuPos.y}px;"
	>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none transition-colors rounded-sm
				{canSplitRight
					? 'text-fg cursor-pointer hover:bg-surface'
					: 'text-muted/50 cursor-default'}"
			disabled={!canSplitRight}
			onclick={() => handleSplit('right')}
		>Split Right</button>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none transition-colors rounded-sm
				{canSplitDown
					? 'text-fg cursor-pointer hover:bg-surface'
					: 'text-muted/50 cursor-default'}"
			disabled={!canSplitDown}
			onclick={() => handleSplit('down')}
		>Split Down</button>
		<div class="h-px bg-edge mx-2 my-1"></div>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none text-danger cursor-pointer hover:bg-danger-bg transition-colors rounded-sm"
			onclick={(e) => {
				if (contextMenuTabId) {
					void handleTabClose(e, contextMenuTabId);
				}
				closeContextMenu();
			}}
		>Close</button>
	</div>
{/if}
