<script lang="ts">
	import { Particles } from '$lib/components/ui/particles'
	import type { Snippet } from 'svelte'

	let {
		children,
		particleColor = '#ffb59f',
		particleQuantity = 75
	}: {
		children: Snippet;
		particleColor?: string;
		particleQuantity?: number;
	} = $props();
</script>

<div class="flex-1 flex items-center justify-center bg-ground relative overflow-hidden">
	<Particles
		class="z-0 opacity-60"
		quantity={particleQuantity}
		color={particleColor}
		size={0.5}
		staticity={40}
		ease={60}
	/>

	<div class="relative z-10 w-full max-w-2xl px-8">
		{@render children()}
	</div>
</div>
