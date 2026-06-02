<!--
  @component
  TitleBarMenu — hamburger fallback for the app menu bar, shown when the
  horizontal menu bar (AppMenuBar) doesn't fit the titlebar. Renders the
  shared buildAppMenu model, so its contents match AppMenuBar exactly.
-->

<script lang="ts">
  import { iconButtonVariants } from '$lib/components/ui/button'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuSub,
    DropdownMenuSubContent,
    DropdownMenuSubTrigger,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { MenuIcon } from '$lib/icons/lucideExports'
  import { buildAppMenu, type AppMenuHandlers } from './menuModel'

  let { onNewProject, onSettings }: AppMenuHandlers = $props()

  let groups = $derived(buildAppMenu({ onNewProject, onSettings }))
</script>

<DropdownMenuRoot>
  <DropdownMenuTrigger aria-label="Menu" class={iconButtonVariants({ size: 'md' })}>
    <MenuIcon size={14} />
  </DropdownMenuTrigger>

  <DropdownMenuContent align="start" sideOffset={6}>
    {#each groups as group, gi (group.label)}
      {#if gi > 0}<DropdownMenuSeparator />{/if}
      {#each group.entries as entry, i (i)}
        {#if entry.kind === 'separator'}
          <DropdownMenuSeparator />
        {:else if entry.kind === 'submenu'}
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{entry.label}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {#each entry.items as item (item.label)}
                <DropdownMenuItem onclick={item.onSelect} disabled={item.disabled}>
                  <span class="truncate" title={item.title}>{item.label}</span>
                </DropdownMenuItem>
              {/each}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        {:else}
          <DropdownMenuItem onclick={entry.onSelect} disabled={entry.disabled}>{entry.label}</DropdownMenuItem>
        {/if}
      {/each}
    {/each}
  </DropdownMenuContent>
</DropdownMenuRoot>
