import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Build straight into the dir rust-embed bakes into the binary.
// In dev, proxy the API (incl. SSE) to a running `blumi web`.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:7777',
        changeOrigin: true,
      },
    },
  },
})
