<!--
  @component
  TreeIndentGuides. One 1px vertical guide per depth step. Used by
  every nested sidebar tree (file tree, issues tree) so they share the
  same visual language.
-->

<script lang="ts">
  import { TREE_INDENT_STEP_PX } from './tree-indent'

  let {
    depth,
    leadingPx = 16
  }: {
    depth: number
    leadingPx?: number
  } = $props()
</script>

{#each Array(depth) as _, i (i)}
  <span
    class="pointer-events-none absolute top-0 bottom-0 w-px bg-subtle/25"
    style="left: {i * TREE_INDENT_STEP_PX + leadingPx}px"
  ></span>
{/each}
