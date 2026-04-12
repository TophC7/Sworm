<script lang="ts">
	import { Menubar } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let {
		class: className,
		children,
		...rest
	}: {
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<Menubar.Trigger
	class={cn(
		'px-2 py-1 text-[0.75rem] text-muted rounded cursor-pointer select-none transition-colors',
		'hover:text-fg hover:bg-raised/60',
		'data-[state=open]:text-fg data-[state=open]:bg-raised/80',
		'outline-none focus-visible:ring-1 focus-visible:ring-accent/40',
		className
	)}
	{...rest}
>
	{#if children}{@render children()}{/if}
</Menubar.Trigger>
