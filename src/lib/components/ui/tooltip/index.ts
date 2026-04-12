import { Tooltip as TooltipPrimitive } from 'bits-ui';

export { default as InfoTooltip } from './InfoTooltip.svelte';
export { default as TooltipContent } from './tooltip-content.svelte';

export const TooltipRoot = TooltipPrimitive.Root;
export const TooltipTrigger = TooltipPrimitive.Trigger;
export const TooltipProvider = TooltipPrimitive.Provider;
export const TooltipPortal = TooltipPrimitive.Portal;
