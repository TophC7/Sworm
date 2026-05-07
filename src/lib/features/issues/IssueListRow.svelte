<!--
  @component
  IssueListRow. One row in a flat list of issues (epic-issue list,
  sub-issue list). Priority chip, mono id, title, status label.
  onSelect fires when the user clicks the row.
-->

<script lang="ts">
  import { priorityToneClass, statusGlyphTone, statusLabel } from './visual'
  import type { Issue } from '$lib/types/backend'

  let {
    issue,
    onSelect
  }: {
    issue: Issue
    onSelect: (issue: Issue) => void
  } = $props()
</script>

<li>
  <button
    type="button"
    class="group flex w-full items-center gap-1.5 px-1 py-1 text-left text-sm hover:bg-surface focus-visible:shadow-focus-ring focus-visible:outline-none"
    onclick={() => onSelect(issue)}
  >
    <span class="shrink-0 font-mono text-2xs {priorityToneClass(issue.priority)}">P{issue.priority}</span>
    <span class="shrink-0 font-mono text-2xs text-subtle">{issue.id}</span>
    <span class="min-w-0 flex-1 truncate group-hover:text-bright">{issue.title}</span>
    <span class="shrink-0 text-2xs {statusGlyphTone(issue.status)}">{statusLabel(issue.status)}</span>
  </button>
</li>
