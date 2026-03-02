import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'jsdom',
  },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': '/src' },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          markdown: [
            'react-markdown',
            'remark-gfm',
            'rehype-highlight',
            'highlight.js',
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
