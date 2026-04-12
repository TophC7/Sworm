<script lang="ts">
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let {
		children,
		class: className,
		gradientSize = 200,
		gradientColor = '#1c1917',
		gradientOpacity = 0.8,
		gradientFrom = '#ffb59f',
		gradientTo = '#763724',
		disabled = false,
		onclick
	}: {
		children?: Snippet;
		class?: string;
		gradientSize?: number;
		gradientColor?: string;
		gradientOpacity?: number;
		gradientFrom?: string;
		gradientTo?: string;
		disabled?: boolean;
		onclick?: () => void;
	} = $props();

	let offScreen = $derived(-gradientSize);
	let mouseX = $state(-200);
	let mouseY = $state(-200);

	function reset() {
		mouseX = offScreen;
		mouseY = offScreen;
	}

	function handlePointerMove(e: PointerEvent) {
		if (disabled) return;
		const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
		mouseX = e.clientX - rect.left;
		mouseY = e.clientY - rect.top;
	}

	let borderBg = $derived(
		`radial-gradient(${gradientSize}px circle at ${mouseX}px ${mouseY}px, ${gradientFrom}, ${gradientTo}, var(--color-edge) 100%)`
	);
	let overlayBg = $derived(
		`radial-gradient(${gradientSize}px circle at ${mouseX}px ${mouseY}px, ${gradientColor}, transparent 100%)`
	);
</script>

<button
	class={cn(
		'group relative rounded-xl text-left transition-transform',
		disabled ? 'opacity-40 cursor-default' : 'cursor-pointer hover:scale-[1.02] active:scale-[0.98]',
		className
	)}
	type="button"
	{disabled}
	onpointermove={handlePointerMove}
	onpointerleave={reset}
	onclick={() => onclick?.()}
>
	<!-- Animated border gradient -->
	<div
		class="pointer-events-none absolute inset-0 rounded-[inherit] opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		style="background: {borderBg};"
		aria-hidden="true"
	></div>

	<!-- Inner background -->
	<div class="absolute inset-px rounded-[inherit] bg-raised" aria-hidden="true"></div>

	<!-- Gradient overlay on hover -->
	<div
		class="pointer-events-none absolute inset-px rounded-[inherit] opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		style="background: {overlayBg}; opacity: {mouseX > 0 ? gradientOpacity : 0};"
		aria-hidden="true"
	></div>

	<!-- Content -->
	<div class="relative">
		{#if children}{@render children()}{/if}
	</div>
</button>
