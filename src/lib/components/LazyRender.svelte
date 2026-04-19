<!--
  @component
  LazyRender -- defers child rendering until the element scrolls into view.

  Uses IntersectionObserver to detect visibility. Once visible, children
  mount and stay mounted (the observer disconnects). A placeholder with
  configurable min-height prevents layout shifts before content appears.

  @param minHeight - placeholder height in px (default 120)
  @param rootMargin - preload margin so content mounts slightly before entering view (default "200px")
-->

<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    children,
    minHeight = 120,
    rootMargin = '200px'
  }: {
    children: Snippet
    minHeight?: number
    rootMargin?: string
  } = $props()

  let el: HTMLDivElement
  let visible = $state(false)

  $effect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          visible = true
          observer.disconnect()
        }
      },
      { rootMargin }
    )
    observer.observe(el)
    return () => observer.disconnect()
  })
</script>

<div bind:this={el}>
  {#if visible}
    {@render children()}
  {:else}
    <div style="min-height: {minHeight}px" class="flex items-center justify-center text-sm text-subtle">
      Scroll to load diff...
    </div>
  {/if}
</div>
