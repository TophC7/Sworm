declare module 'monaco-editor/esm/vs/base/common/codicons.js' {
  export const Codicon: Record<string, unknown>
}

declare module 'monaco-editor/esm/vs/platform/actions/common/actions.js' {
  export const MenuId: {
    DiffEditorHunkToolbar: unknown
    DiffEditorSelectionToolbar: unknown
  }
  export const MenuRegistry: {
    appendMenuItem(id: unknown, item: unknown): { dispose(): void }
  }
}

declare module 'monaco-editor/esm/vs/platform/contextkey/common/contextkey.js' {
  export const ContextKeyExpr: {
    has(key: string): unknown
    equals(key: string, value: unknown): { negate(): unknown }
    and(...expr: unknown[]): unknown
  }
}

declare module 'monaco-editor/esm/vs/editor/standalone/browser/standaloneServices.js' {
  export const StandaloneServices: {
    get(service: unknown): unknown
  }
}

declare module 'monaco-editor/esm/vs/platform/keybinding/common/keybinding.js' {
  export const IKeybindingService: unknown
}
