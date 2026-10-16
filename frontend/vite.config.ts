import path from 'node:path'
// `vitest/config` re-exports Vite's `defineConfig` widened with the `test`
// field, so one config file still drives both `vite build` and `vitest`.
import { defineConfig } from 'vitest/config'
import { searchForWorkspaceRoot } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const HOST = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: HOST || false,
    hmr: HOST
      ? { protocol: 'ws', host: HOST, port: 1421 }
      : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
    fs: {
      // The ingest userscript lives with the Rust backend that serves it;
      // its tests (`src/lib/userscript.test.ts`) load it from there.
      allow: [
        searchForWorkspaceRoot(process.cwd()),
        path.resolve(__dirname, '../src/capture/ingest'),
      ],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'jsdom',
    // Note: not `src/test/` — the repo's .gitignore has a bare `test` rule
    // that matches a directory of that name at any depth.
    setupFiles: ['./src/testing/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
