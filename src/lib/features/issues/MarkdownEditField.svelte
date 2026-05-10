<!--
  @component
  MarkdownEditField. Click-to-edit markdown body used by issue and epic
  detail surfaces. Reads as a rendered MarkdownRenderer card; click or
  focus enters a Textarea with a Done button that returns to render
  mode. Owns its editing toggle so callers only bind the value.
-->

<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Textarea } from '$lib/components/ui/input'
  import MarkdownRenderer from '$lib/components/markdown/MarkdownRenderer.svelte'
  import { PencilIcon } from '$lib/icons/lucideExports'

  let {
    value = $bindable(''),
    placeholder = 'Click to add a description.',
    editPlaceholder,
    rows = 8
  }: {
    value: string
    placeholder?: string
    editPlaceholder?: string
    rows?: number
  } = $props()

  let editing = $state(false)
</script>

{#if editing}
  <div class="flex flex-col gap-2">
    <Textarea {rows} bind:value placeholder={editPlaceholder ?? placeholder} class="text-base" />
    <div class="flex justify-end gap-1.5">
      <Button size="xs" variant="ghost" onclick={() => (editing = false)}>Done</Button>
    </div>
  </div>
{:else}
  <button
    type="button"
    class="group relative flex w-full flex-col items-stretch overflow-hidden rounded-md border border-edge bg-surface text-left transition-colors hover:border-accent/40 focus-visible:shadow-focus-ring focus-visible:outline-none"
    onclick={() => (editing = true)}
    title="Click to edit description"
  >
    {#if value.trim()}
      <MarkdownRenderer source={value} />
    {:else}
      <span class="px-6 py-5 text-base text-subtle italic">{placeholder}</span>
    {/if}
    <span
      class="absolute top-2 right-2 flex items-center gap-1 rounded bg-raised px-1.5 py-0.5 text-2xs text-muted opacity-0 transition-opacity group-hover:opacity-100"
    >
      <PencilIcon size={10} />
      Edit
    </span>
  </button>
{/if}
