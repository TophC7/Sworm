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
		class={cn(
			'flex items-center justify-between px-2.5 py-1.5 border-b border-edge bg-surface shrink-0',
			className
		)}
		data-sidebar="header"
	>
		{#if children}{@render children()}{/if}
	</div>
{/if}
