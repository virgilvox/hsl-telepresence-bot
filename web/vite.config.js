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
  },
})
