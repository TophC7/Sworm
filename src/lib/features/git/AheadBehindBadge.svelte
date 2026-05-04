<!--
  @component
  AheadBehindBadge: compact ahead/behind counters for git branches.
-->

<script lang="ts">
  import { ArrowDown, ArrowUp } from '$lib/icons/lucideExports'

  type BadgeSize = 'xs' | 'sm'

  let {
    ahead,
    behind,
    size = 'sm',
    twoColor = false
  }: {
    ahead: number
    behind: number
    size?: BadgeSize
    twoColor?: boolean
  } = $props()

  let iconSize = $derived(size === 'xs' ? 9 : 12)
  let hasBoth = $derived(ahead > 0 && behind > 0)

  function tone(kind: 'ahead' | 'behind'): string {
    if (!twoColor && hasBoth) return 'text-warning'
    return kind === 'ahead' ? 'text-success' : 'text-danger'
  }
</script>

{#if ahead > 0 || behind > 0}
  <span class="inline-flex shrink-0 items-center gap-1 font-mono text-2xs">
    {#if ahead > 0}
      <span class="inline-flex items-center gap-0.5 {tone('ahead')}" title="{ahead} ahead">
        <ArrowUp size={iconSize} />{ahead}
      </span>
    {/if}
    {#if behind > 0}
      <span
        class="inline-flex items-center gap-0.5 {tone('behind')}"
        title="{behind} behind"
      >
        <ArrowDown size={iconSize} />{behind}
      </span>
    {/if}
  </span>
{/if}
