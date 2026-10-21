import { defineConfig } from "vite";

// Tauri expects a fixed port in development and plain static files in production.
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2021", outDir: "dist", emptyOutDir: true },
});
