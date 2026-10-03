<!--
  @component
  MaskIcon — paints a monochrome SVG in the current text color through a CSS mask.
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    src,
    width,
    height = width,
    label,
    class: className
  }: { src: string; width: number; height?: number; label: string; class?: string } = $props()

  // Quoted: builds inline small SVGs as data URIs, which contain single quotes and parentheses.
  let mask = $derived(`url("${src}") no-repeat center / contain`)
</script>

<span
  class={cn('inline-block shrink-0 bg-current', className)}
  style="width: {width}px; height: {height}px; -webkit-mask: {mask}; mask: {mask};"
  role="img"
  aria-label={label}
></span>
