<!--
  @component
  TitleBarMenu — the app's hamburger menu, rendered from buildAppMenu.
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
  import { buildAppMenu } from './menuModel'

  let entries = $derived(buildAppMenu())
</script>

<DropdownMenuRoot>
  <DropdownMenuTrigger aria-label="Menu" class={iconButtonVariants({ size: 'md' })}>
    <MenuIcon size={14} />
  </DropdownMenuTrigger>

  <DropdownMenuContent align="start" sideOffset={6}>
    {#each entries as entry, i (i)}
      {#if entry.kind === 'separator'}
        <DropdownMenuSeparator />
      {:else if entry.kind === 'submenu'}
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>{entry.label}</DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            {#each entry.items as item (item.title ?? item.label)}
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
  </DropdownMenuContent>
</DropdownMenuRoot>
