import { Menubar as MenubarPrimitive } from 'bits-ui';

export { default as MenubarContent } from './menubar-content.svelte';
export { default as MenubarItem } from './menubar-item.svelte';
export { default as MenubarSeparator } from './menubar-separator.svelte';
export { default as MenubarSubContent } from './menubar-sub-content.svelte';
export { default as MenubarTrigger } from './menubar-trigger.svelte';

export const MenubarRoot = MenubarPrimitive.Root;
export const MenubarMenu = MenubarPrimitive.Menu;
export const MenubarSub = MenubarPrimitive.Sub;
export const MenubarSubTrigger = MenubarPrimitive.SubTrigger;
export const MenubarGroup = MenubarPrimitive.Group;
export const MenubarPortal = MenubarPrimitive.Portal;
