<!--
  @component
  Placeholder rendered in place of a Monaco diff for binary or
  oversized files. Keeps the row height tiny and the shell chrome
  consistent — no editor is mounted.
-->

<script lang="ts">
  import { AlertTriangle } from '$lib/icons/lucideExports'

  let {
    reason = 'Binary file — content not shown'
  }: {
    reason?: string
  } = $props()
</script>

<div class="flex items-center gap-2 px-4 py-6 text-sm text-subtle">
  <AlertTriangle size={14} class="shrink-0 text-muted" />
  <span>{reason}</span>
</div>
