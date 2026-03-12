import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy:
      process.env.NODE_ENV === "development"
        ? {
            "/api": "http://localhost:3000",
            "/ws": { target: "ws://localhost:3000", ws: true },
          }
        : {
            "/api": {
              target: "http://localhost:3000",
              changeOrigin: true,
            },
            "/ws": {
              target: "ws://localhost:3000",
              ws: true,
              changeOrigin: true,
            },
          },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks: {
          hljs: ["highlight.js"],
          "react-vendor": ["react", "react-dom"],
        },
      },
    },
  },
});
