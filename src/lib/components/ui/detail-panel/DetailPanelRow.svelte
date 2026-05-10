<!--
  @component
  DetailPanelRow. Labeled metadata row inside a DetailPanel. Renders an
  uppercase tracker label (with optional leading icon snippet) above the
  body content. `dense` tightens the label/body gap from 1.5 to 1, used
  for single-line read-only rows like Created/Updated.
-->

<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    label,
    icon,
    dense = false,
    children
  }: {
    label: string
    icon?: Snippet
    dense?: boolean
    children: Snippet
  } = $props()
</script>

<div class="flex flex-col border-b border-edge px-3 py-2.5 last:border-b-0 {dense ? 'gap-1' : 'gap-1.5'}">
  <span class="flex items-center gap-1 text-2xs font-medium tracking-wider text-muted uppercase">
    {#if icon}{@render icon()}{/if}
    {label}
  </span>
  {@render children()}
</div>
