<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    children,
    class: className,
    duration = 0.4,
    delay = 0,
    offset = 6,
    direction = 'up',
    blur = '6px'
  }: {
    children: Snippet
    class?: string
    duration?: number
    delay?: number
    offset?: number
    direction?: 'up' | 'down' | 'left' | 'right'
    blur?: string
  } = $props()

  let visible = $state(false)

  // Compute transform axis and direction
  let axis = $derived(direction === 'left' || direction === 'right' ? 'X' : 'Y')
  let sign = $derived(direction === 'down' || direction === 'right' ? -1 : 1)

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
		filter: blur({visible ? '0px' : blur});
		transform: translate{axis}({visible ? 0 : sign * offset}px);
		transition: opacity {duration}s ease-out {delay}s, filter {duration}s ease-out {delay}s, transform {duration}s ease-out {delay}s;
	"
>
  {@render children()}
</div>
