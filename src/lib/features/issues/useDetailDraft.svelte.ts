import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/utils/client-error'

export function useDetailDraft<D extends object, P extends Record<keyof D, unknown>>(opts: {
  seed: D
  normalize: (d: D) => P
  save: (patch: Partial<P>) => Promise<void>
}): {
  readonly drafts: D
  readonly dirty: boolean
  readonly saving: boolean
  save(): Promise<void>
} {
  const drafts = $state<D>({ ...opts.seed })
  let baseline = $state.raw(opts.normalize(opts.seed))
  let saving = $state(false)
  const normalized = $derived(opts.normalize(drafts))
  const patch = $derived.by(() => {
    const changed: Partial<P> = {}
    for (const key of Object.keys(normalized) as (keyof P)[]) {
      if (!shallowEq(normalized[key], baseline[key])) changed[key] = normalized[key]
    }
    return changed
  })
  const dirty = $derived(Object.keys(patch).length > 0)

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
      const sent = patch
      if (saving || !Object.keys(sent).length) return
      saving = true
      try {
        await opts.save(sent)
        baseline = { ...baseline, ...sent }
      } catch (error) {
        notify.error('Save failed', getErrorMessage(error))
      } finally {
        saving = false
      }
    }
  }
}

function shallowEq(a: unknown, b: unknown): boolean {
  return a === b || (Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((v, i) => v === b[i]))
}
