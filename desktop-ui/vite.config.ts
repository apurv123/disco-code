import { defineConfig } from "vite"
import solid from "vite-plugin-solid"

export default defineConfig({
  plugins: [solid()],
  // Tauri serves the built assets from a custom protocol, so asset URLs must be
  // relative rather than rooted at the server origin.
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "esnext",
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
  },
})