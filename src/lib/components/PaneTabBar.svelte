<script lang="ts">
	import type { Tab, TabId, PaneSlot } from '$lib/stores/workspace.svelte';
	import {
		setActiveTab,
		closeTab,
		splitPane,
		moveTabToPane,
		collapsePaneIfEmpty
	} from '$lib/stores/workspace.svelte';
	import X from '@lucide/svelte/icons/x';
	import TabBeam from '$lib/components/ui/tab-beam.svelte';

	let {
		tabs,
		activeTabId,
		paneSlot,
		projectId,
		isFocused = false
	}: {
		tabs: Tab[];
		activeTabId: TabId | null;
		paneSlot: PaneSlot;
		projectId: string;
		isFocused?: boolean;
	} = $props();

	let contextMenuOpen = $state(false);
	let contextMenuTabId = $state<TabId | null>(null);
	let contextMenuPos = $state({ x: 0, y: 0 });

	function handleTabClick(tabId: TabId) {
		setActiveTab(projectId, paneSlot, tabId);
	}

	function handleTabClose(e: Event, tabId: TabId) {
		e.stopPropagation();
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
		const newSlot = splitPane(projectId, direction);
		if (newSlot) {
			moveTabToPane(projectId, contextMenuTabId, newSlot);
		}
		closeContextMenu();
	}

	function tabLabel(tab: Tab): string {
		if (tab.kind === 'session') return tab.title;
		return tab.filePath.split('/').pop() ?? tab.filePath;
	}

	function handleTablistKeydown(e: KeyboardEvent) {
		if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
		const tabEls = Array.from((e.currentTarget as HTMLElement).querySelectorAll('[role="tab"]'));
		const current = tabEls.indexOf(e.target as HTMLElement);
		if (current === -1) return;
		e.preventDefault();
		const next = e.key === 'ArrowRight'
			? (current + 1) % tabEls.length
			: (current - 1 + tabEls.length) % tabEls.length;
		(tabEls[next] as HTMLElement).focus();
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

{#if tabs.length > 0}
	<div class="flex items-center bg-ground border-b shrink-0 min-h-7 overflow-x-auto scrollbar-none relative {isFocused ? 'border-edge' : 'border-edge/50'}"
		role="tablist" aria-label="Pane tabs" tabindex="-1" onkeydown={handleTablistKeydown}
	>

		{#each tabs as tab (tab.id)}
			<button
				class="group relative flex items-center gap-1 px-2.5 h-7 shrink-0 border-none cursor-pointer text-[0.72rem] transition-colors
					{activeTabId === tab.id
						? 'bg-surface text-fg'
						: 'bg-transparent text-muted hover:text-fg hover:bg-surface/50'}"
				role="tab"
				aria-selected={activeTabId === tab.id}
				onclick={() => handleTabClick(tab.id)}
				oncontextmenu={(e) => handleContextMenu(e, tab.id)}
			>
				{#if activeTabId === tab.id && isFocused}<TabBeam />{/if}
				<span class="truncate max-w-[120px] {tab.kind === 'diff' && tab.temporary ? 'italic' : ''}">
					{tabLabel(tab)}
				</span>
				<span
					class="opacity-0 group-hover:opacity-100 text-muted hover:text-danger transition-all leading-none"
					role="button"
					tabindex="0"
					onclick={(e) => handleTabClose(e, tab.id)}
					onkeydown={(e) => e.key === 'Enter' && handleTabClose(e, tab.id)}
				>
					<X size={10} />
				</span>
			</button>
		{/each}
	</div>
{/if}

<!-- Context menu -->
{#if contextMenuOpen}
	<div class="fixed inset-0 z-40" onclick={closeContextMenu} role="none"></div>
	<div
		class="fixed z-50 min-w-[140px] bg-raised border border-edge rounded-lg shadow-[0_8px_24px_rgba(0,0,0,0.4)] py-1 text-[0.78rem]"
		style="left: {contextMenuPos.x}px; top: {contextMenuPos.y}px;"
	>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none text-fg cursor-pointer hover:bg-surface transition-colors rounded-sm"
			onclick={() => handleSplit('right')}
		>Split Right</button>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none text-fg cursor-pointer hover:bg-surface transition-colors rounded-sm"
			onclick={() => handleSplit('down')}
		>Split Down</button>
		<div class="h-px bg-edge mx-2 my-1"></div>
		<button
			class="w-full px-3 py-1.5 text-left bg-transparent border-none text-danger cursor-pointer hover:bg-danger-bg transition-colors rounded-sm"
			onclick={(e) => { if (contextMenuTabId) handleTabClose(e, contextMenuTabId); closeContextMenu(); }}
		>Close</button>
	</div>
{/if}
