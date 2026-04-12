<script lang="ts">
	import { Tabs } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let { class: className, children, ...rest }: { class?: string; children?: Snippet } = $props();
</script>

<Tabs.List
	class={cn(
		'inline-flex items-center bg-ground rounded overflow-hidden border border-edge',
		className
	)}
	{...rest}
>
	{#if children}{@render children()}{/if}
</Tabs.List>
