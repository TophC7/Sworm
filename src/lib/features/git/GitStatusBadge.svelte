<!--
  @component
  GitStatusBadge — colored status letter for git file changes (M, U, D, R, A).
-->

<script lang="ts">
  import { gitStatusDisplay, gitStatusColor, gitStatusLabel } from '$lib/features/git/gitStatus'

  let { status }: { status: string } = $props()
</script>

<span class="shrink-0 pr-2 text-center font-mono font-bold {gitStatusColor(status)}" title={gitStatusLabel(status)}>
  {gitStatusDisplay(status)}
</span>
