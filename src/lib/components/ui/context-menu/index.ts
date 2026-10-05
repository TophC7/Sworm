import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'

export { default as ContextMenuContent } from './context-menu-content.svelte'
export { default as ContextMenuItem } from './context-menu-item.svelte'
export { default as ContextMenuSeparator } from './context-menu-separator.svelte'

export const ContextMenuRoot = ContextMenuPrimitive.Root
export const ContextMenuTrigger = ContextMenuPrimitive.Trigger
