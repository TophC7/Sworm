<!--
  @component
  CommandPill — titlebar trigger for the CommandCenter palette.

  Keeps a subtle 6px radius so it previews the rounded palette it opens,
  while all other titlebar chrome is flat (rounded-none).

  Reads the active binding for `toggle-command-palette` so the displayed
  kbd hint follows any user rebind. Do not hardcode.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const commandPillVariants = tv({
    base:
      'inline-flex h-[26px] items-center gap-2.5 rounded-md border border-edge bg-raised pl-2.5 pr-2 ' +
      'text-xs text-muted cursor-pointer transition-colors ' +
      'hover:border-accent hover:text-bright ' +
      'focus-visible:shadow-focus-ring focus-visible:outline-none'
  })

  export type CommandPillVariant = VariantProps<typeof commandPillVariants>
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { SearchIcon } from '$lib/icons/lucideExports'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { splitShortcut } from '$lib/features/command-palette/shortcuts/keybindings.svelte'
  import { getEffectiveSpec } from '$lib/features/command-palette/shortcuts/overrides.svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  let {
    class: className,
    label = 'Search commands\u2026',
    onclick,
    ...rest
  }: HTMLButtonAttributes & { class?: string; label?: string } = $props()

  // Live-read the active binding so a rebind updates the hint in place.
  let shortcutParts = $derived(splitShortcut(getEffectiveSpec('toggle-command-palette', 'Ctrl+Shift+P')))
</script>

<button
  data-slot="command-pill"
  type="button"
  class={cn(commandPillVariants(), 'min-w-[220px]', className)}
  {onclick}
  {...rest}
>
  <SearchIcon size={12} />
  <span class="truncate">{label}</span>
  <KbdGroup class="ml-auto">
    {#each shortcutParts as part (part)}
      <Kbd class="rounded-sm">{part}</Kbd>
    {/each}
  </KbdGroup>
</button>
