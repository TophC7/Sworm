<!--
  @component
  ShortcutPreview: renders a keybinding sequence as Sworm keycaps.

  @param spec - normalized or user-facing shortcut sequence
  @param large - larger keycaps for active recording states
  @param class - optional wrapper classes
-->

<script lang="ts">
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { splitShortcutSequence } from '$lib/features/command-palette/shortcuts/spec'
  import { cn } from '$lib/utils/cn'

  let {
    spec,
    large = false,
    class: className
  }: {
    spec: string
    large?: boolean
    class?: string
  } = $props()

  const sequence = $derived(splitShortcutSequence(spec))
</script>

<span class={cn('flex flex-wrap items-center gap-1', className)}>
  {#each sequence as stroke, strokeIndex (strokeIndex)}
    {#if strokeIndex > 0}<span class="text-2xs text-subtle">then</span>{/if}
    <KbdGroup class={large ? 'text-lg' : ''}>
      {#each stroke as part, i (i)}
        {#if i > 0}<span class="text-subtle">+</span>{/if}
        <Kbd class={large ? 'h-8 min-w-8 px-2 text-md' : ''}>{part}</Kbd>
      {/each}
    </KbdGroup>
  {/each}
</span>
