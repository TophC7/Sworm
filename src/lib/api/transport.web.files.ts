export interface FileReadRequest {
  requestId: string
  projectPath: string
  filePath: string
  version: string
  size: number
}

export interface FileBytes {
  bytes: Uint8Array<ArrayBuffer>
  version: string
}

export const MAX_FILE_BYTES = 256 * 1024 * 1024
const MAX_CHUNK_BYTES = 1024 * 1024

/** Owns exactly one approved-size buffer. A digest cannot be cancelled; settled tracks its actual lifetime. */
export class FileAssembly {
  private readonly bytes: Uint8Array<ArrayBuffer>
  private received = 0
  private completed = false
  private cancelled = false
  private rejectCancelled: ((reason: Error) => void) | null = null
  settled: Promise<void> = Promise.resolve()

  constructor(
    size: number,
    private readonly digest: (bytes: Uint8Array<ArrayBuffer>) => Promise<ArrayBuffer> = (bytes) =>
      crypto.subtle.digest('SHA-256', bytes)
  ) {
    if (!Number.isSafeInteger(size) || size < 0 || size > MAX_FILE_BYTES) {
      throw new Error('Invalid argument: Invalid approved file size')
    }
    this.bytes = new Uint8Array(size)
  }

  append(chunk: Uint8Array): void {
    if (this.cancelled) throw new Error('Invalid argument: File read cancelled')
    if (this.completed || chunk.length > MAX_CHUNK_BYTES || chunk.length > this.bytes.length - this.received) {
      throw new Error('File read size mismatch')
    }
    this.bytes.set(chunk, this.received)
    this.received += chunk.length
  }

  get bytesReceived(): number {
    return this.received
  }

  get isComplete(): boolean {
    return this.completed
  }

  complete(returnedVersion: string): Promise<FileBytes> {
    if (this.completed) return Promise.reject(new Error('File read completed twice'))
    this.completed = true // Close may arrive while the native digest is pending.
    if (this.cancelled) return Promise.reject(new Error('Invalid argument: File read cancelled'))
    if (this.received !== this.bytes.length) return Promise.reject(new Error('File read size mismatch'))
    // Open already checked the approved metadata identity; the terminal version is the content digest.
    const digestResult = Promise.resolve()
      .then(() => this.digest(this.bytes))
      .then((hash) => {
        if (!(hash instanceof ArrayBuffer)) throw new Error('File read hash failed')
        const hex = Array.from(new Uint8Array(hash), (byte) => byte.toString(16).padStart(2, '0')).join('')
        if (hex !== returnedVersion) throw new Error('File read hash mismatch')
        return { bytes: this.bytes, version: returnedVersion }
      })
    this.settled = digestResult.then(
      () => {},
      () => {}
    )
    const cancelled = new Promise<never>((_, reject) => {
      this.rejectCancelled = reject
    })
    return Promise.race([digestResult, cancelled]).finally(() => {
      this.rejectCancelled = null
    })
  }

  cancel(): void {
    this.cancelled = true
    this.rejectCancelled?.(new Error('Invalid argument: File read cancelled'))
  }
}
