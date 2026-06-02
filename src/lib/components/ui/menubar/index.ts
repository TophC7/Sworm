import { Menubar as MenubarPrimitive } from 'bits-ui'

export { default as MenubarContent } from './menubar-content.svelte'
export { default as MenubarTrigger } from './menubar-trigger.svelte'

export const MenubarRoot = MenubarPrimitive.Root
export const MenubarMenu = MenubarPrimitive.Menu

// In bits-ui the item / separator / sub parts are the same shared `menu/`
// components across every menu variant, so the dropdown-menu wrappers
// render correctly inside a Menubar.Menu. Re-export them under Menubar
// names instead of duplicating identical styled wrappers.
export {
  DropdownMenuItem as MenubarItem,
  DropdownMenuSeparator as MenubarSeparator,
  DropdownMenuSub as MenubarSub,
  DropdownMenuSubContent as MenubarSubContent,
  DropdownMenuSubTrigger as MenubarSubTrigger
} from '$lib/components/ui/dropdown-menu'
