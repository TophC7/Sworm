<!--
  @component
  AppMenuBar — horizontal File / View / Help menu bar for the titlebar.
  Built on the Menubar primitive; its contents come from the shared
  buildAppMenu model so it never drifts from the hamburger fallback
  (TitleBarMenu). Shown only while it fits the bar.
-->

<script lang="ts">
  import {
    MenubarRoot,
    MenubarMenu,
    MenubarTrigger,
    MenubarContent,
    MenubarItem,
    MenubarSeparator,
    MenubarSub,
    MenubarSubContent,
    MenubarSubTrigger
  } from '$lib/components/ui/menubar'
  import { buildAppMenu, type AppMenuHandlers } from './menuModel'

  let { onNewProject, onSettings }: AppMenuHandlers = $props()

  let groups = $derived(buildAppMenu({ onNewProject, onSettings }))
</script>

<MenubarRoot class="flex items-center gap-0.5">
  {#each groups as group (group.label)}
    <MenubarMenu>
      <MenubarTrigger>{group.label}</MenubarTrigger>
      <MenubarContent>
        {#each group.entries as entry, i (i)}
          {#if entry.kind === 'separator'}
            <MenubarSeparator />
          {:else if entry.kind === 'submenu'}
            <MenubarSub>
              <MenubarSubTrigger>{entry.label}</MenubarSubTrigger>
              <MenubarSubContent>
                {#each entry.items as item (item.label)}
                  <MenubarItem onclick={item.onSelect} disabled={item.disabled}>
                    <span class="truncate" title={item.title}>{item.label}</span>
                  </MenubarItem>
                {/each}
              </MenubarSubContent>
            </MenubarSub>
          {:else}
            <MenubarItem onclick={entry.onSelect} disabled={entry.disabled}>{entry.label}</MenubarItem>
          {/if}
        {/each}
      </MenubarContent>
    </MenubarMenu>
  {/each}
</MenubarRoot>
