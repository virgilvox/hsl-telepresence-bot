import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// GitHub Pages serves a project site from /<repo>/, so that build needs a base
// path. The Pages workflow sets VITE_BASE; local dev and any root-path host
// (DigitalOcean App Platform) fall back to '/'.
export default defineConfig({
  base: process.env.VITE_BASE || '/',
  plugins: [vue()],
  build: {
    target: 'es2022',
    outDir: 'dist',
    rollupOptions: {
      input: {
        // The operator console.
        main: fileURLToPath(new URL('index.html', import.meta.url)),
        // A stand-in robot, so the console can be driven with no hardware.
        // Shipping it alongside means testing is a URL rather than a checkout.
        sim: fileURLToPath(new URL('sim.html', import.meta.url)),
      },
    },
  },
})
