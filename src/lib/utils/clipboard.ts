import { platform } from '$lib/platform'

export async function copyToClipboard(text: string): Promise<void> {
  await platform.clipboard.writeText(text)
}
