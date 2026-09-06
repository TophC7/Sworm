<!--
  @component
  MarkdownEditField. Click-to-edit markdown body used by issue and epic
  detail surfaces. Reads as a rendered MarkdownRenderer card; click or
  the Edit button enters a Textarea with a Done button that returns to
  render mode. Owns its editing toggle so callers only bind the value.
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
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div
    class="group relative flex w-full flex-col items-stretch overflow-hidden rounded-md border border-edge bg-surface text-left transition-colors hover:border-accent/40"
    onclick={(event) => {
      if (event.target instanceof Element && event.target.closest('a, summary, input, button')) return
      editing = true
    }}
  >
    {#if value.trim()}
      <MarkdownRenderer source={value} />
    {:else}
      <span class="px-6 py-5 text-base text-subtle italic">{placeholder}</span>
    {/if}
    <div class="absolute top-2 right-2 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
      <Button size="xs" variant="ghost" onclick={() => (editing = true)} aria-label="Edit description">
        <PencilIcon size={10} />
        Edit
      </Button>
    </div>
  </div>
{/if}
