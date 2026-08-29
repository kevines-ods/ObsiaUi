import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Port fixe attendu par tauri.conf.json (devUrl : http://localhost:1420)
  server: {
    port: 1420,
    strictPort: true,
  },
})
