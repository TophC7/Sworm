<!--
  TabBeam: animated gradient glow that sweeps across the top of an active tab.
  Place as a child inside the active tab button, positioned absolutely.
-->
<script lang="ts">
	import { cn } from '$lib/utils/cn';

	let {
		class: className
	}: {
		class?: string;
	} = $props();
</script>

<span
	class={cn(
		'absolute top-0 left-0 right-0 h-[2px] overflow-hidden pointer-events-none',
		className
	)}
	aria-hidden="true"
>
	<span class="tab-beam-gradient"></span>
</span>

<style>
	/* The beam: a traveling gradient that sweeps left to right */
	.tab-beam-gradient {
		position: absolute;
		inset: 0;
		/* Static glow base */
		background: var(--color-accent);
	}

	.tab-beam-gradient::after {
		content: '';
		position: absolute;
		top: 0;
		left: 0;
		width: 200%;
		height: 100%;
		background: linear-gradient(
			90deg,
			transparent 0%,
			var(--color-accent-dim) 20%,
			var(--color-accent-bright) 40%,
			var(--color-max) 50%,
			var(--color-accent-bright) 60%,
			var(--color-accent-dim) 80%,
			transparent 100%
		);
		animation: beam-sweep 3s ease-in-out infinite;
	}

	@keyframes beam-sweep {
		0% {
			transform: translateX(-50%);
		}
		100% {
			transform: translateX(0%);
		}
	}
</style>
