import path from 'node:path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'jsdom',
    // Each worker loads jsdom and the app graph; bound resource contention
    // rather than relaxing the existing per-test timeouts.
    maxWorkers: 2,
    setupFiles: ['./src/test/setup.ts'],
  },
})
