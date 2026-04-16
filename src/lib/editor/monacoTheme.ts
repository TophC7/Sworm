// Sworm Monaco theme — maps the warm base16 palette from app.css
// to Monaco's editor and token color system.
//
// Token colors mirror the --shiki-* variables so syntax looks
// consistent between the diff viewer (Shiki) and the editor (Monaco).

import type * as Monaco from 'monaco-editor'

export const SWORM_THEME_NAME = 'sworm'

export function registerSwormTheme(monaco: typeof Monaco) {
  monaco.editor.defineTheme(SWORM_THEME_NAME, {
    base: 'vs-dark',
    inherit: false,
    rules: [
      // -- Base tokens --
      { token: '', foreground: 'e2e2e2', background: '131313' },
      { token: 'comment', foreground: '6e6864', fontStyle: 'italic' },
      { token: 'keyword', foreground: 'd6acff' },
      { token: 'keyword.control', foreground: 'd6acff' },
      { token: 'storage', foreground: 'd6acff' },
      { token: 'storage.type', foreground: 'd6acff' },

      // -- Strings --
      { token: 'string', foreground: '98ff7f' },
      { token: 'string.escape', foreground: 'b7ffa5' },
      { token: 'regexp', foreground: 'b7ffa5' },

      // -- Constants --
      { token: 'number', foreground: 'ffae57' },
      { token: 'constant', foreground: 'ffae57' },
      { token: 'constant.language', foreground: 'ffae57' },

      // -- Functions --
      { token: 'entity.name.function', foreground: '7fbaff' },
      { token: 'support.function', foreground: '7fbaff' },
      { token: 'meta.function-call', foreground: '7fbaff' },

      // -- Types / classes --
      { token: 'entity.name.type', foreground: 'ffc0ad' },
      { token: 'support.type', foreground: 'ffc0ad' },
      { token: 'entity.name.class', foreground: 'ffc0ad' },
      { token: 'type', foreground: 'ffc0ad' },
      { token: 'type.identifier', foreground: 'ffc0ad' },

      // -- Variables / parameters --
      { token: 'variable', foreground: 'e2e2e2' },
      { token: 'variable.parameter', foreground: 'ffddd3' },
      { token: 'parameter', foreground: 'ffddd3' },

      // -- Punctuation / operators --
      { token: 'delimiter', foreground: 'a59c99' },
      { token: 'delimiter.bracket', foreground: 'a59c99' },
      { token: 'operator', foreground: 'a59c99' },

      // -- Tags (HTML/XML) --
      { token: 'tag', foreground: 'ff7672' },
      { token: 'tag.attribute.name', foreground: 'ffae57' },
      { token: 'attribute.name', foreground: 'ffae57' },
      { token: 'attribute.value', foreground: '98ff7f' },

      // -- Markdown --
      { token: 'markup.heading', foreground: 'fff3ef', fontStyle: 'bold' },
      { token: 'markup.bold', fontStyle: 'bold' },
      { token: 'markup.italic', fontStyle: 'italic' },
      { token: 'markup.underline.link', foreground: '9effef' },

      // -- JSON keys --
      { token: 'string.key.json', foreground: '7fbaff' },
      { token: 'string.value.json', foreground: '98ff7f' },

      // -- CSS --
      { token: 'attribute.name.css', foreground: '7fbaff' },
      { token: 'attribute.value.css', foreground: '98ff7f' },
      { token: 'attribute.value.number.css', foreground: 'ffae57' },
      { token: 'attribute.value.unit.css', foreground: 'ffae57' }
    ],
    colors: {
      // -- Editor chrome --
      'editor.background': '#131313',
      'editor.foreground': '#e2e2e2',
      'editor.lineHighlightBackground': '#1c191780',
      'editor.selectionBackground': '#7c2d1560',
      'editor.inactiveSelectionBackground': '#7c2d1530',
      'editor.selectionHighlightBackground': '#7c2d1520',
      'editorCursor.foreground': '#ffb59f',

      // -- Line numbers --
      'editorLineNumber.foreground': '#6e6864',
      'editorLineNumber.activeForeground': '#a59c99',

      // -- Gutter --
      'editorGutter.background': '#131313',

      // -- Indentation guides --
      'editorIndentGuide.background': '#3a322e40',
      'editorIndentGuide.activeBackground': '#3a322e80',

      // -- Bracket matching --
      'editorBracketMatch.background': '#7c2d1530',
      'editorBracketMatch.border': '#ffb59f60',

      // -- Widget chrome (autocomplete, hover) --
      'editorWidget.background': '#262220',
      'editorWidget.border': '#3a322e',
      'editorSuggestWidget.background': '#262220',
      'editorSuggestWidget.border': '#3a322e',
      'editorSuggestWidget.selectedBackground': '#3a322e',
      'editorSuggestWidget.highlightForeground': '#ffb59f',
      'editorHoverWidget.background': '#262220',
      'editorHoverWidget.border': '#3a322e',

      // -- Find / replace --
      'editor.findMatchBackground': '#7c2d1540',
      'editor.findMatchHighlightBackground': '#7c2d1520',

      // -- Scrollbar --
      'scrollbar.shadow': '#00000000',
      'scrollbarSlider.background': '#3a322e80',
      'scrollbarSlider.hoverBackground': '#4a403a80',
      'scrollbarSlider.activeBackground': '#4a403aa0',

      // -- Minimap (disabled, but just in case) --
      'minimap.background': '#131313',

      // -- Misc --
      'editorOverviewRuler.border': '#00000000',
      focusBorder: '#ffb59f40'
    }
  })
}
