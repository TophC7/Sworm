// Web URL scheme for durable workbenches: `/?workbench=<id>` hosts one
// workbench. An unknown id is created on arrival; `/` mints a fresh one.

export const WORKBENCH_PARAM = 'workbench'

export function workbenchHref(id: string): string {
  return `/?${new URLSearchParams({ [WORKBENCH_PARAM]: id })}`
}
