<!--
  @component
  PanelHeader — reusable panel header with left/right content areas.

  Provides consistent header chrome (height, border, background) for panels
  and sidebars. Used by SidebarPanel; available for any view that needs a
  standard header bar.

  @param left - optional snippet rendered on the left
  @param right - optional snippet rendered on the right
  @param class - optional extra classes
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    left,
    right,
    class: className
  }: {
    left?: Snippet
    right?: Snippet
    class?: string
  } = $props()
</script>

<div
  class={cn('flex h-8 min-h-8 shrink-0 items-center justify-between border-b border-edge bg-surface px-2.5', className)}
>
  {#if left}
    <div class="flex items-center gap-1.5">
      {@render left()}
    </div>
  {/if}
  {#if right}
    <div class="flex items-center gap-1">
      {@render right()}
    </div>
  {/if}
</div>
