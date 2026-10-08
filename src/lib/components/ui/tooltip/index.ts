import { Tooltip as Primitive } from 'bits-ui'

export { default as Content } from './TooltipContent.svelte'

export const Root = Primitive.Root
export const Trigger = Primitive.Trigger
export const Provider = Primitive.Provider

// Standalone; import by name next to the namespace.
export { default as InfoTooltip } from './InfoTooltip.svelte'
