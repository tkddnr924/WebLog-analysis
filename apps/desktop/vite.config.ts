import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri가 devUrl로 기대하는 포트를 고정한다.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", sourcemap: false },
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
