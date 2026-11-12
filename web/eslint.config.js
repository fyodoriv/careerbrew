import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  { ignores: ['dist'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],
    },
  },
  {
    // The browser suite runs on a seeded server with a pinned clock: derive dates from NOW (playwright.config.ts),
    // and the visual tests from VISUAL_NOW (playwright.visual.config.ts, the one place that fixes their day).
    files: ['e2e/**/*.ts', 'visual/**/*.ts'],
    rules: {
      'no-restricted-syntax': ['error', ...[
        'Literal[value=/\\d{4}-\\d{2}-\\d{2}/]',
        'Literal[regex.pattern=/\\d{4}-\\d{2}-\\d{2}/]',
        'TemplateElement[value.raw=/\\d{4}-\\d{2}-\\d{2}/]',
      ].map((selector) => ({ selector, message: 'No fixed dates in e2e tests: derive them from NOW, so the suite passes on any day.' }))],
    },
  },
)
