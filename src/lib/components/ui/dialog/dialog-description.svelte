<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let { class: className, children, ...rest }: { class?: string; children?: Snippet } = $props();
</script>

<Dialog.Description class={cn('text-muted leading-relaxed whitespace-pre-line', className)} {...rest}>
	{#if children}{@render children()}{/if}
</Dialog.Description>
