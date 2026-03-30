import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    coverage: {
      exclude: [
        'dist/**',
        'public/**',
        'src/**/__tests__/**',
        'src/**/*.test.*',
        'src/main.tsx',
        'src/vite-env.d.ts',
        '*.config.*',
      ],
      provider: 'v8',
      reporter: ['text', 'text-summary'],
      thresholds: {
        'src/hooks/**': {
          branches: 65,
          functions: 65,
          lines: 35,
          statements: 35,
        },
        'src/lib/**': {
          branches: 80,
          functions: 80,
          lines: 80,
          statements: 80,
        },
        'src/store/**': {
          branches: 80,
          functions: 80,
          lines: 80,
          statements: 80,
        },
      },
    },
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': '/src' },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          editor: [
            '@tiptap/react',
            '@tiptap/starter-kit',
            '@tiptap/core',
          ],
          markdown: [
            'react-markdown',
            'remark-gfm',
            'rehype-highlight',
          ],
        },
      },
    },
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:3200',
        changeOrigin: true,
      },
    },
  },
})
