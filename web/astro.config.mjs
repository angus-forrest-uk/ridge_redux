// @ts-check
import { defineConfig } from "astro/config";
import solid from "@astrojs/solid-js";

// The Rust server serves dist/ and the API. In `astro dev`, the API calls are
// proxied to it, so run the server alongside (`just run`).
const backend = process.env.RIDGE_BACKEND ?? "http://127.0.0.1:8420";

export default defineConfig({
  integrations: [solid()],
  vite: {
    server: {
      proxy: {
        "/api": backend,
        "/healthz": backend,
      },
    },
  },
});
