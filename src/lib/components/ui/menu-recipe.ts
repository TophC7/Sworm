export const menuContentClass =
  'z-50 min-w-[180px] rounded-lg border border-edge bg-raised py-1 text-base shadow-popover'

export const menuSeparatorClass = 'mx-2 my-1 h-px bg-edge'

export function menuItemClass(destructive: boolean, disabled: boolean): string {
  return (
    'flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-left outline-none focus-visible:shadow-focus-ring ' +
    (disabled
      ? 'cursor-not-allowed text-muted/50'
      : destructive
        ? 'cursor-pointer text-danger hover:bg-danger-bg focus:bg-danger-bg'
        : 'cursor-pointer text-fg hover:bg-surface focus:bg-surface')
  )
}
