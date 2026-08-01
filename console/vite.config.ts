import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Served from the site root today. The console's links go through the injected `PathResolver`
// (see `src/routing.ts`), so if this ever moves under a base path there is exactly one place to
// say so — `base` here, and the resolver reads it.
export default defineConfig({
  base: '/',
  plugins: [vue()],
})
