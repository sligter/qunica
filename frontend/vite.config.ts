import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const reactPackages = new Set(['react', 'react-dom', 'react-router', 'react-router-dom', 'scheduler'])
const statePackages = new Set(['@tanstack/react-query', 'zustand'])
const formPackages = new Set(['@hookform/resolvers', 'react-hook-form', 'zod'])
const utilityPackages = new Set([
  'class-variance-authority',
  'clsx',
  'tailwind-merge',
  '@microsoft/fetch-event-source',
])

function packageNameFromId(id: string): string | undefined {
  const normalizedId = id.replace(/\\/g, '/')
  const pathParts = normalizedId.split('/node_modules/')
  const nodeModulePath = pathParts.length > 1 ? pathParts[pathParts.length - 1] : undefined
  if (!nodeModulePath) return undefined

  const [first, second] = nodeModulePath.split('/')
  return first.startsWith('@') ? `${first}/${second}` : first
}

function manualChunks(id: string): string | undefined {
  const packageName = packageNameFromId(id)
  if (!packageName) return undefined

  if (reactPackages.has(packageName)) return 'vendor-react'
  if (statePackages.has(packageName)) return 'vendor-state'
  if (formPackages.has(packageName)) return 'vendor-forms'
  if (packageName === 'lucide-react') return 'vendor-icons'
  if (packageName.startsWith('@radix-ui/')) return 'vendor-radix'
  if (packageName.startsWith('@tauri-apps/')) return 'vendor-tauri'
  if (utilityPackages.has(packageName)) return 'vendor-utils'
  return 'vendor'
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    host: '0.0.0.0',
    port: 5173,
    proxy: {
      '/api/v2': {
        target: 'http://localhost:8000',
        changeOrigin: true,
      },
      '/ws': {
        target: 'ws://localhost:8000',
        ws: true,
        changeOrigin: true,
      },
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks,
      },
    },
  },
})
