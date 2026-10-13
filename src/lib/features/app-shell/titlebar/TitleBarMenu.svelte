<!--
  @component
  TitleBarMenu — the app's hamburger menu, rendered from buildAppMenu.
-->

<script lang="ts">
  import { iconButtonVariants } from '$lib/components/ui/button'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { MenuIcon } from '$lib/icons/lucideExports'
  import { buildAppMenu } from './menuModel'

  let entries = $derived(buildAppMenu())
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger aria-label="Menu" class={iconButtonVariants({ size: 'md' })}>
    <MenuIcon size={14} />
  </DropdownMenu.Trigger>

  <DropdownMenu.Content align="start" sideOffset={6}>
    {#each entries as entry, i (i)}
      {#if entry.kind === 'separator'}
        <DropdownMenu.Separator />
      {:else}
        <DropdownMenu.Item onclick={entry.onSelect} disabled={entry.disabled}>
          <span>{entry.label}</span>
          {#if entry.shortcut}
            <DropdownMenu.Shortcut>{entry.shortcut}</DropdownMenu.Shortcut>
          {/if}
        </DropdownMenu.Item>
      {/if}
    {/each}
  </DropdownMenu.Content>
</DropdownMenu.Root>
