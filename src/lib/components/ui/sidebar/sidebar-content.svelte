<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils/cn';
	import { useSidebar } from './sidebar-state.svelte';

	let {
		class: className,
		children
	}: {
		class?: string;
		children?: Snippet;
	} = $props();

	const sidebar = useSidebar();
</script>

{#if sidebar.open}
	<div
		class={cn('flex-1 flex flex-col overflow-hidden min-h-0', className)}
		data-sidebar="content"
	>
		{#if children}{@render children()}{/if}
	</div>
{/if}
