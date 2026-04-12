<script lang="ts">
	import { getSessions } from '$lib/stores/sessions.svelte';
	import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte';
	import { Button } from '$lib/components/ui/button';
	import Circle from '@lucide/svelte/icons/circle';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';

	let sessions = $derived(getSessions());
	let liveSessions = $derived(sessions.filter((s) => s.status === 'running'));
	let zoom = $derived(getZoomLevel());
</script>

<footer class="flex items-center justify-end px-3 py-0.5 bg-surface border-t border-edge text-[0.68rem] shrink-0 min-h-6 gap-3">
	<div class="flex items-center gap-2.5">
		{#if liveSessions.length > 0}
			<span class="flex items-center gap-1 text-success">
				<Circle size={6} fill="currentColor" />
				{liveSessions.length} live
			</span>
		{/if}
		{#if liveSessions.length > 1}
			<span class="flex items-center gap-1 text-warning" title="Sessions share the same working tree">
				<AlertTriangle size={10} /> shared
			</span>
		{/if}

		<div class="flex items-center gap-0.5 text-muted">
			<Button variant="ghost" size="icon-sm" onclick={zoomOut} title="Zoom out">
				<Minus size={10} />
			</Button>
			<button
				class="bg-transparent border-none text-muted cursor-pointer text-[0.65rem] hover:text-fg transition-colors px-0.5 min-w-6 text-center"
				onclick={zoomReset}
				title="Reset zoom"
			>{Math.round(zoom * 100)}%</button>
			<Button variant="ghost" size="icon-sm" onclick={zoomIn} title="Zoom in">
				<Plus size={10} />
			</Button>
		</div>
	</div>
</footer>
