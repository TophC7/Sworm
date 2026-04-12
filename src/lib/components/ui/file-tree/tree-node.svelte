<script lang="ts">
	import { slide } from 'svelte/transition';
	import { cubicOut } from 'svelte/easing';
	import type { Snippet } from 'svelte';

	let {
		expanded = true,
		depth = 0,
		children,
		label
	}: {
		expanded?: boolean;
		depth?: number;
		children?: Snippet;
		label?: Snippet;
	} = $props();
</script>

<div>
	{#if label}
		{@render label()}
	{/if}

	{#if expanded && children}
		<div
			transition:slide={{ duration: 150, easing: cubicOut }}
		>
			{@render children()}
		</div>
	{/if}
</div>
