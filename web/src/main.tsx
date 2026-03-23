import '@/index.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { Toaster } from 'sonner'

import { App } from '@/app'
import { initTheme } from '@/lib/theme'

// init dark mode from user preference or system preference, and listen for changes
initTheme()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
      <Toaster position="top-right" richColors />
    </BrowserRouter>
  </StrictMode>
)
