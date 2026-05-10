<!--
  @component
  DetailPanel. Rounded surface card used by issue/epic detail sidebars
  to host stacked DetailPanelRow children. Children render their own
  bottom border; the last child auto-strips it via `last:border-b-0`.
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let { class: className, children }: { class?: string; children: Snippet } = $props()
</script>

<div class={cn('flex flex-col rounded-lg border border-edge bg-surface', className)}>
  {@render children()}
</div>
