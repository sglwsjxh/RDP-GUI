// SPDX-License-Identifier: AGPL-3.0-only
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true
  },
  watch: {
    ignored: ['**/modules/**', '**/target/**', '**/main.rs', '**/build.rs']
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true
  },
  publicDir: '../public'
})
