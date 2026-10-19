import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'

import '@/i18n'
import App from './App.tsx'
import { queryClient } from './lib/queryClient'
import './index.css'
import { AndroidShell } from './components/layout/AndroidShell'
import { isAndroidRuntime } from './lib/androidSession'
import { apiOrigin, isDesktopRuntime } from './lib/runtime'
import { DesktopShell } from './components/layout/DesktopShell'
import { applyConnectionPolicy } from './lib/connectionPolicy'

if (isDesktopRuntime()) applyConnectionPolicy(apiOrigin())

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      {isAndroidRuntime() ? <AndroidShell><App /></AndroidShell> : isDesktopRuntime() ? <DesktopShell><App /></DesktopShell> : <App />}
    </QueryClientProvider>
  </StrictMode>,
)
