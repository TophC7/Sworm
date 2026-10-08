<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    children,
    class: className,
    duration = 0.4,
    delay = 0,
    offset = 8
  }: {
    children: Snippet
    class?: string
    duration?: number
    delay?: number
    offset?: number
  } = $props()

  let visible = $state(false)

  $effect(() => {
    // Trigger animation on next frame after mount
    const id = requestAnimationFrame(() => {
      visible = true
    })
    return () => cancelAnimationFrame(id)
  })
</script>

<div
  class={cn(className)}
  style="
		opacity: {visible ? 1 : 0};
		filter: blur({visible ? '0px' : '6px'});
		transform: translateY({visible ? 0 : offset}px);
		transition: opacity {duration}s ease-out {delay}s, filter {duration}s ease-out {delay}s, transform {duration}s ease-out {delay}s;
	"
>
  {@render children()}
</div>
