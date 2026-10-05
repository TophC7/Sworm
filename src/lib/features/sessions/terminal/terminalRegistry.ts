import type { TerminalTransferState } from '$lib/types/backend'

interface RegistryTerminal {
  detach(): void
  release(): void
  dispose(): void
  focus(): void
  exportTransferState(): Promise<TerminalTransferState>
  importTransferState(state: TerminalTransferState, transferId: string): Promise<void>
}

export function createTerminalRegistry<I, T extends RegistryTerminal>(
  label: string,
  keyOf: (init: I) => string,
  create: (init: I) => T
) {
  const terminals = new Map<string, T>()

  function getOrCreate(init: I): T {
    const key = keyOf(init)
    let terminal = terminals.get(key)
    if (!terminal) {
      terminal = create(init)
      terminals.set(key, terminal)
    }
    return terminal
  }

  return {
    getOrCreate,
    get: (key: string): T | undefined => terminals.get(key),
    detach: (key: string): void => terminals.get(key)?.detach(),
    focus: (key: string): void => terminals.get(key)?.focus(),
    release(key: string): void {
      const terminal = terminals.get(key)
      if (!terminal) return
      terminal.release()
      terminals.delete(key)
    },
    dispose(key: string): void {
      const terminal = terminals.get(key)
      if (!terminal) return
      terminal.dispose()
      terminals.delete(key)
    },
    releaseAll(): void {
      for (const terminal of terminals.values()) terminal.release()
      terminals.clear()
    },
    async exportTransferState(key: string): Promise<TerminalTransferState> {
      const terminal = terminals.get(key)
      if (!terminal) throw new Error(`Unknown ${label} ${key}`)
      return terminal.exportTransferState()
    },
    async importTransferState(init: I, state: TerminalTransferState, transferId: string): Promise<void> {
      const terminal = getOrCreate(init)
      try {
        await terminal.importTransferState(state, transferId)
      } catch (error) {
        terminal.release()
        terminals.delete(keyOf(init))
        throw error
      }
    }
  }
}
