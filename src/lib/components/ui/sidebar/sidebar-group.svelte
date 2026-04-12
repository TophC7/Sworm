<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils/cn';

	let {
		class: className,
		children
	}: {
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={cn('py-1', className)} data-sidebar="group">
	{#if children}{@render children()}{/if}
</div>
