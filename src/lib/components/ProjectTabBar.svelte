<script lang="ts">
	import { getProjectById } from '$lib/stores/projects.svelte';
	import {
		getOpenProjectIds,
		getActiveProjectId,
		selectProject,
		closeProject,
		reorderProjects
	} from '$lib/stores/workspace.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';

	import X from '@lucide/svelte/icons/x';
	import Plus from '@lucide/svelte/icons/plus';
	import TabBeam from '$lib/components/ui/tab-beam.svelte';

	let { onAddProject }: { onAddProject: () => void } = $props();

	let openIds = $derived(getOpenProjectIds());
	let activeId = $derived(getActiveProjectId());

	let confirmClose = $state<string | null>(null);
	let confirmProject = $derived(confirmClose ? getProjectById(confirmClose) : null);

	// Drag-to-reorder state
	let dragIndex = $state<number | null>(null);

	function handleSelect(id: string) {
		selectProject(id);
	}

	function handleClose(e: Event, id: string) {
		e.stopPropagation();
		confirmClose = id;
	}

	function doClose() {
		if (confirmClose) {
			closeProject(confirmClose);
			confirmClose = null;
		}
	}

	// Pointer-based drag reorder
	function handleDragStart(e: DragEvent, index: number) {
		dragIndex = index;
		if (e.dataTransfer) {
			e.dataTransfer.effectAllowed = 'move';
			e.dataTransfer.setData('text/plain', String(index));
		}
	}

	function handleDragOver(e: DragEvent, index: number) {
		e.preventDefault();
		if (dragIndex !== null && dragIndex !== index) {
			reorderProjects(dragIndex, index);
			dragIndex = index;
		}
	}

	function handleDragEnd() {
		dragIndex = null;
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

<div class="flex items-center min-w-0 flex-1 overflow-x-auto scrollbar-none gap-px" role="tablist" aria-label="Open projects" tabindex="-1" onkeydown={handleTablistKeydown}>
	{#each openIds as id, index (id)}
		{@const project = getProjectById(id)}
		{#if project}
			<button
				class="group relative flex items-center gap-1.5 px-3 h-8 shrink-0 border-none cursor-pointer text-[0.78rem] rounded-t transition-colors
					{activeId === id
						? 'bg-ground text-bright'
						: 'bg-transparent text-muted hover:text-fg hover:bg-surface/50'}"
				role="tab"
				aria-selected={activeId === id}
				onclick={() => handleSelect(id)}
				draggable="true"
				ondragstart={(e) => handleDragStart(e, index)}
				ondragover={(e) => handleDragOver(e, index)}
				ondragend={handleDragEnd}
			>
				{#if activeId === id}<TabBeam />{/if}
				<span class="truncate max-w-[140px]">{project.name}</span>
				<span
					class="text-[0.7rem] opacity-0 group-hover:opacity-100 text-muted hover:text-danger transition-all leading-none"
					role="button"
					tabindex="0"
					onclick={(e) => handleClose(e, id)}
					onkeydown={(e) => e.key === 'Enter' && handleClose(e, id)}
				><X size={10} /></span>
			</button>
		{/if}
	{/each}

	<button
		class="flex items-center justify-center w-7 h-7 shrink-0 bg-transparent border-none text-muted cursor-pointer text-sm hover:text-bright transition-colors ml-0.5"
		onclick={onAddProject}
		title="Open project"
	><Plus size={14} /></button>
</div>

<ConfirmDialog
	open={confirmClose !== null}
	title="Close Project"
	message="Close {confirmProject?.name ?? 'this project'}? Running sessions will be stopped."
	confirmLabel="Close"
	onCancel={() => (confirmClose = null)}
	onConfirm={doClose}
/>
