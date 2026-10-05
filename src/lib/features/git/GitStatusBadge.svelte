<!--
  @component
  GitStatusBadge — colored status letter for git file changes (M, U, D, R, A).
-->

<script lang="ts">
  import { gitStatusMeta } from '$lib/features/git/gitStatus'

  let { status }: { status: string } = $props()
  let meta = $derived(gitStatusMeta(status))
</script>

<span class="shrink-0 pr-2 text-center font-mono font-bold {meta.color}" title={meta.label}>
  {meta.letter}
</span>
