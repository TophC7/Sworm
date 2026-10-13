import { tv } from 'tailwind-variants'

// Shared by the dropdown and context menu wrappers. State comes from data attributes
// (bits sets data-highlighted/data-disabled; Item sets data-inset/data-destructive), so
// consumers never pass state into the recipe.

export const menuContentVariants = tv({
  base: 'z-50 min-w-[180px] overflow-x-hidden overflow-y-auto rounded-lg border border-edge bg-raised py-1 text-sm shadow-popover outline-none',
  variants: {
    // bits names the available-height var per primitive; sub-content uses the generic menu one.
    source: {
      dropdown: 'max-h-(--bits-dropdown-menu-content-available-height)',
      context: 'max-h-(--bits-context-menu-content-available-height)',
      sub: 'max-h-(--bits-menu-content-available-height)'
    }
  }
})

// No automatic svg size: consumers size lucide icons with `size={…}`, which a CSS size would override.
// `not-data-disabled` guards fills: an item disabled while highlighted keeps data-highlighted until blur.
export const menuItemClass = [
  'relative flex cursor-pointer items-center gap-2 rounded-sm px-3 py-1.5 text-fg outline-none select-none focus-visible:shadow-focus-ring',
  'not-data-disabled:data-highlighted:bg-surface',
  'data-disabled:cursor-not-allowed data-disabled:text-muted/50',
  'data-inset:ps-8',
  "[&_svg]:shrink-0 not-data-disabled:[&_svg:not([class*='text-'])]:text-muted",
  // The svg selector mirrors line above so the extra data-destructive attribute wins on specificity.
  "data-destructive:not-data-disabled:text-danger data-destructive:not-data-disabled:data-highlighted:bg-danger-bg data-destructive:not-data-disabled:[&_svg:not([class*='text-'])]:text-danger"
].join(' ')

/** Checkbox/radio rows: text starts at ps-8 so it lines up with `inset` items. */
export const menuIndicatorItemClass = `${menuItemClass} ps-8`

/** Absolute check/radio slot sitting in the item's left padding. */
export const menuIndicatorClass =
  'pointer-events-none absolute start-3 flex size-3.5 items-center justify-center'

export const menuGroupHeadingClass =
  'px-3 py-1 text-xs font-medium tracking-wide text-muted uppercase data-inset:ps-8'

export const menuShortcutClass = 'ms-auto ps-6 text-xs text-subtle'

export const menuSeparatorClass = 'mx-2 my-1 h-px bg-edge'
