// Nix language support for Monaco: configuration and snippets.
//
// Adapted from vscode-nix-ide. Called once during Monaco init after the
// 'nix' language ID is registered via Shiki.

/** Register language config and snippets for Nix. */
export function registerNixLanguage(monaco: typeof import('monaco-editor')) {
  // ── Language configuration ──────────────────────────────────────
  monaco.languages.setLanguageConfiguration('nix', {
    comments: {
      lineComment: '#',
      blockComment: ['/*', '*/']
    },
    brackets: [
      ['${', '}'],
      ['{', '}'],
      ['[', ']'],
      ['(', ')']
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"', notIn: ['string'] },
      { open: "''", close: "''" }
    ],
    surroundingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" }
    ],
    indentationRules: {
      increaseIndentPattern: /^\s*(let|if|then|else|with|rec)\b.*$/,
      decreaseIndentPattern: /^\s*(in|else)\b.*$/
    },
    wordPattern: /(-?\d*\.\d\w*)|([^\`\~\!\@\#\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)/g
  })

  // ── Snippets ────────────────────────────────────────────────────
  const snippets = [
    {
      label: 'if',
      insertText: 'if ${1:condition} then ${2:value} else ${0:fallback}',
      documentation: 'Conditional expression'
    },
    { label: 'let', insertText: 'let\n  ${1:binding} = ${2:value};\nin\n  ${0}', documentation: 'Let expression' },
    { label: 'rec', insertText: 'rec {\n  ${0}\n}', documentation: 'Recursive attribute set' },
    { label: 'with', insertText: 'with ${1:expr};\n${0}', documentation: 'With expression' }
  ]

  monaco.languages.registerCompletionItemProvider('nix', {
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position)
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn
      }
      return {
        suggestions: snippets.map((s) => ({
          label: s.label,
          kind: monaco.languages.CompletionItemKind.Snippet,
          insertText: s.insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          documentation: s.documentation,
          range
        }))
      }
    }
  })
}
