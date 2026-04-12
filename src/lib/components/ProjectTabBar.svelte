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
	import Plus from '@lucide/svelte/icons/plus';
	import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs';

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

	function handleAuxClick(e: MouseEvent, id: string) {
		if (e.button !== 1) return;
		handleClose(e, id);
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

</script>

<TabStrip variant="project" ariaLabel="Open projects">
	{#each openIds as id, index (id)}
		{@const project = getProjectById(id)}
		{#if project}
			<TabButton
				variant="project"
				active={activeId === id}
				onclick={() => handleSelect(id)}
				onauxclick={(e) => handleAuxClick(e, id)}
				onClose={(e) => handleClose(e, id)}
				draggable="true"
				ondragstart={(e) => handleDragStart(e, index)}
				ondragover={(e) => handleDragOver(e, index)}
				ondragend={handleDragEnd}
			>
				<span class="truncate max-w-[140px]">{project.name}</span>
			</TabButton>
		{/if}
	{/each}

	{#snippet trailing()}
		<button
			class="flex items-center justify-center w-7 h-7 shrink-0 bg-transparent border-none text-muted cursor-pointer text-sm hover:text-bright transition-colors ml-0.5"
			onclick={onAddProject}
			title="Open project"
		><Plus size={14} /></button>
	{/snippet}
</TabStrip>

<ConfirmDialog
	open={confirmClose !== null}
	title="Close Project"
	message="Close {confirmProject?.name ?? 'this project'}? Running sessions will be stopped."
	confirmLabel="Close"
	onCancel={() => (confirmClose = null)}
	onConfirm={doClose}
/>
