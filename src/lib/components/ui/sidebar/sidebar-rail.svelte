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

<!-- Rail: shown when collapsed in icon mode, clicking expands -->
{#if !sidebar.open && sidebar.collapsible === 'icon'}
	<button
		class={cn(
			'w-full h-full flex flex-col items-center pt-3 cursor-pointer bg-transparent border-none',
			'hover:bg-surface transition-colors',
			className
		)}
		onclick={() => sidebar.toggle()}
		title="Expand sidebar"
		data-sidebar="rail"
	>
		{#if children}{@render children()}{/if}
	</button>
{/if}
