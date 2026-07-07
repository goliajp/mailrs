import eslintJs from '@eslint/js'
import eslintConfigPrettier from 'eslint-config-prettier'
import eslintPluginJsxA11y from 'eslint-plugin-jsx-a11y'
import { configs as pftConfigs } from 'eslint-plugin-perfectionist'
import eslintPluginReact from 'eslint-plugin-react'
import eslintPluginReactHooks from 'eslint-plugin-react-hooks'
import eslintPluginReactRefresh from 'eslint-plugin-react-refresh'
import unusedImports from 'eslint-plugin-unused-imports'
import globals from 'globals'
import eslintTs from 'typescript-eslint'

export default eslintTs.config(
  { ignores: ['dist', 'src/lib/types.openapi.ts'] },

  // base
  eslintJs.configs.recommended,
  ...eslintTs.configs.recommended,
  pftConfigs['recommended-natural'],
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.es2021 },
      parserOptions: {
        ecmaFeatures: { jsx: true },
        ecmaVersion: 12,
        sourceType: 'module',
      },
    },
    plugins: { 'unused-imports': unusedImports },
    rules: {
      semi: [2, 'never'],
      'no-const-assign': 2,
      'no-console': 0,
      'no-param-reassign': 0,
      'no-shadow': 0,
      'no-unused-expressions': 0,
      'no-unused-vars': 'off',
      'consistent-return': 0,
      indent: 0,
      'comma-dangle': 0,
      'max-len': 0,
      camelcase: 0,
      '@typescript-eslint/no-explicit-any': 0,
      '@typescript-eslint/no-unused-expressions': 0,
      '@typescript-eslint/no-unused-vars': 'off',
      // Every admin list endpoint returns `{items:[...]}`; typing it as
      // a bare array via `fetchJson<T[]>` is a lie and blows up at
      // runtime. Use `fetchList<T>` from `@/lib/api` — it handles both
      // the enveloped and bare shapes and returns `T[]`.
      'no-restricted-syntax': [
        'error',
        {
          selector:
            'CallExpression[callee.name="fetchJson"] > TSTypeParameterInstantiation > TSArrayType',
          message:
            'Do not use fetchJson<T[]>. Use fetchList<T>() from @/lib/api — the backend wraps list responses as {items: T[]} and a bare-array type crashes on .map at runtime.',
        },
      ],
      'unused-imports/no-unused-imports': 'error',
      'unused-imports/no-unused-vars': [
        'error',
        {
          args: 'after-used',
          argsIgnorePattern: '^_',
          vars: 'all',
          varsIgnorePattern: '^_',
        },
      ],
      'perfectionist/sort-imports': [
        'error',
        {
          groups: [
            'side-effect',
            'style',
            'type',
            ['builtin', 'external'],
            ['internal', 'tsconfig-path'],
            ['parent', 'sibling', 'index'],
            'unknown',
          ],
          internalPattern: ['^@/.+'],
          order: 'asc',
          type: 'natural',
        },
      ],
      'perfectionist/sort-interfaces': [
        'error',
        {
          groups: ['member', 'method', 'unknown'],
          order: 'asc',
          partitionByComment: true,
          partitionByNewLine: true,
          type: 'natural',
        },
      ],
      'perfectionist/sort-objects': [
        'error',
        {
          groups: ['property', 'method', 'unknown'],
          order: 'asc',
          partitionByComment: true,
          partitionByNewLine: true,
          type: 'natural',
        },
      ],
    },
  },

  // react
  {
    settings: {
      react: { version: 'detect' },
    },
  },
  {
    plugins: { 'jsx-a11y': eslintPluginJsxA11y },
    rules: {
      'jsx-a11y/anchor-is-valid': 0,
      'jsx-a11y/click-events-have-key-events': 0,
      'jsx-a11y/no-static-element-interactions': 0,
    },
  },
  {
    plugins: { react: eslintPluginReact },
    rules: {
      'react/function-component-definition': 0,
      'react/jsx-curly-brace-presence': 2,
      'react/jsx-curly-newline': 0,
      'react/jsx-filename-extension': 0,
      'react/jsx-one-expression-per-line': 0,
      'react/jsx-props-no-spreading': 0,
      'react/no-array-index-key': 0,
      'react/no-unstable-nested-components': 0,
      'react/prop-types': 0,
      'react/react-in-jsx-scope': 0,
    },
  },
  {
    plugins: { 'react-hooks': eslintPluginReactHooks },
    rules: {
      'react-hooks/exhaustive-deps': 'error',
      'react-hooks/rules-of-hooks': 'error',
    },
  },
  {
    plugins: { 'react-refresh': eslintPluginReactRefresh },
    rules: {
      'react-refresh/only-export-components': [
        'error',
        { allowConstantExport: true },
      ],
    },
  },

  // prettier must be last
  eslintConfigPrettier,
)
