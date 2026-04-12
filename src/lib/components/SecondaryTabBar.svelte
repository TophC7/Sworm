<script lang="ts">
	import { getSessions, getActiveSessionId, selectSession, removeSession } from '$lib/stores/sessions.svelte';
	import { getActiveProjectId } from '$lib/stores/projects.svelte';
	import { statusIcon, statusColorClass } from '$lib/utils/session';
	import X from '@lucide/svelte/icons/x';
	import Plus from '@lucide/svelte/icons/plus';
	import TabBeam from '$lib/components/ui/tab-beam.svelte';

	let {
		onNewSession,
		onSelectSession,
		activeDiffFile = null,
		diffTemporary = true,
		onSelectDiff,
		onCloseDiff,
		onPersistDiff
	}: {
		onNewSession: () => void;
		onSelectSession?: () => void;
		activeDiffFile?: string | null;
		diffTemporary?: boolean;
		onSelectDiff?: () => void;
		onCloseDiff?: () => void;
		onPersistDiff?: () => void;
	} = $props();

	let sessions = $derived(getSessions());
	let activeSessionId = $derived(getActiveSessionId());
	let activeProjectId = $derived(getActiveProjectId());

	function handleSessionClose(e: Event, sessionId: string) {
		e.stopPropagation();
		if (!activeProjectId) return;
		void removeSession(sessionId, activeProjectId);
	}

	function handleTablistKeydown(e: KeyboardEvent) {
		if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
		const tabs = Array.from((e.currentTarget as HTMLElement).querySelectorAll('[role="tab"]'));
		const current = tabs.indexOf(e.target as HTMLElement);
		if (current === -1) return;
		e.preventDefault();
		const next = e.key === 'ArrowRight'
			? (current + 1) % tabs.length
			: (current - 1 + tabs.length) % tabs.length;
		(tabs[next] as HTMLElement).focus();
	}
</script>

<div class="flex items-center bg-ground border-b border-edge shrink-0 min-h-8 overflow-x-auto scrollbar-none" role="tablist" aria-label="Sessions" tabindex="-1" onkeydown={handleTablistKeydown}>
	{#each sessions as session (session.id)}
		<button
			class="group relative flex items-center gap-1.5 px-3 h-8 shrink-0 border-none cursor-pointer text-[0.75rem] transition-colors
				{activeSessionId === session.id
					? 'bg-surface text-bright'
					: 'bg-transparent text-muted hover:text-fg hover:bg-surface/50'}"
			role="tab"
			aria-selected={activeSessionId === session.id}
			onclick={() => { selectSession(session.id); onSelectSession?.(); }}
		>
			{#if activeSessionId === session.id}<TabBeam />{/if}
			<span class="text-[0.55rem] shrink-0 {statusColorClass(session.status)}">
				{statusIcon(session.status)}
			</span>
			<span class="truncate max-w-[120px]">{session.title}</span>
			<span
				class="text-[0.65rem] opacity-0 group-hover:opacity-100 text-muted hover:text-danger transition-all leading-none"
				role="button"
				tabindex="0"
				onclick={(e) => handleSessionClose(e, session.id)}
				onkeydown={(e) => e.key === 'Enter' && handleSessionClose(e, session.id)}
			><X size={10} /></span>
		</button>
	{/each}

	<!-- Active diff tab -->
	{#if activeDiffFile}
		<button
			class="group relative flex items-center gap-1.5 px-3 h-8 shrink-0 border-none cursor-pointer text-[0.75rem] transition-colors bg-surface text-bright"
			role="tab"
			aria-selected={true}
			onclick={() => onSelectDiff?.()}
			ondblclick={() => onPersistDiff?.()}
		>
			<TabBeam />
			<span class="truncate max-w-[140px] font-mono text-[0.7rem] {diffTemporary ? 'italic' : ''}">{activeDiffFile}</span>
			<span
				class="text-[0.65rem] opacity-0 group-hover:opacity-100 text-muted hover:text-danger transition-all leading-none"
				role="button"
				tabindex="0"
				onclick={(e) => { e.stopPropagation(); onCloseDiff?.(); }}
				onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); onCloseDiff?.(); } }}
			><X size={10} /></span>
		</button>
	{/if}

	<!-- Sticky + button -->
	<button
		class="flex items-center justify-center w-7 h-7 shrink-0 bg-transparent border-none text-muted cursor-pointer text-sm hover:text-bright transition-colors ml-0.5 sticky right-0 bg-ground"
		onclick={onNewSession}
		title="New session"
	><Plus size={14} /></button>
</div>
