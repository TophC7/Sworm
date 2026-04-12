import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'

export { default as DropdownMenuContent } from './dropdown-menu-content.svelte'
export { default as DropdownMenuItem } from './dropdown-menu-item.svelte'
export { default as DropdownMenuSeparator } from './dropdown-menu-separator.svelte'
export { default as DropdownMenuSubContent } from './dropdown-menu-sub-content.svelte'

export const DropdownMenuRoot = DropdownMenuPrimitive.Root
export const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger
export const DropdownMenuSub = DropdownMenuPrimitive.Sub
export const DropdownMenuSubTrigger = DropdownMenuPrimitive.SubTrigger
export const DropdownMenuGroup = DropdownMenuPrimitive.Group
