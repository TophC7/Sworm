// useDetailDraft: shared scaffolding for editor surfaces that hold a
// dirty-tracked draft against an immutable baseline (e.g. issue / epic
// detail forms). Designed to be instantiated inside a child component
// that the parent gates with {#key entity.id}, so $state initializers
// run fresh on each id change and we never mirror state via $effect.

export type DetailDraft<D> = {
  readonly drafts: D
  readonly dirty: boolean
  readonly saving: boolean
  save: () => Promise<void>
  reset: () => void
}

type DetailDraftOptions<D extends object> = {
  initial: D
  // Optional override for the dirty check. Defaults to a shallow per-key
  // comparison against the snapshot of `initial`. Use this when a
  // field needs trimmed / numeric / array-equality semantics.
  isDirty?: (drafts: D, baseline: D) => boolean
  save: (drafts: D) => Promise<void>
  onSaved?: () => void
}

/**
 * Build a draft store seeded from `opts.initial`.
 *
 * The returned object exposes reactive `drafts`, `dirty`, and `saving`
 * via getters so the host component can use them inside `$derived` and
 * template expressions.
 */
export function useDetailDraft<D extends object>(opts: DetailDraftOptions<D>): DetailDraft<D> {
  const baseline = { ...opts.initial }
  let drafts = $state<D>({ ...opts.initial })
  let saving = $state(false)

  const isDirty = opts.isDirty ?? defaultIsDirty<D>

  const dirty = $derived(isDirty(drafts, baseline))

  return {
    get drafts() {
      return drafts
    },
    get dirty() {
      return dirty
    },
    get saving() {
      return saving
    },
    async save() {
      saving = true
      try {
        await opts.save(drafts)
        opts.onSaved?.()
      } finally {
        saving = false
      }
    },
    reset() {
      drafts = { ...baseline }
    }
  }
}

function defaultIsDirty<D extends object>(drafts: D, baseline: D): boolean {
  for (const key of Object.keys(baseline) as (keyof D)[]) {
    if (!shallowEq(drafts[key], baseline[key])) return true
  }
  return false
}

function shallowEq(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) return false
    }
    return true
  }
  return false
}
