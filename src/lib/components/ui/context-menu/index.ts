import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'

export { default as ContextMenuContent } from './context-menu-content.svelte'
export { default as ContextMenuItem } from './context-menu-item.svelte'
export { default as ContextMenuSeparator } from './context-menu-separator.svelte'
export { default as ContextMenuSubContent } from './context-menu-sub-content.svelte'
export { default as ContextMenuSubTrigger } from './context-menu-sub-trigger.svelte'

export const ContextMenuRoot = ContextMenuPrimitive.Root
export const ContextMenuTrigger = ContextMenuPrimitive.Trigger
export const ContextMenuSub = ContextMenuPrimitive.Sub
export const ContextMenuGroup = ContextMenuPrimitive.Group
