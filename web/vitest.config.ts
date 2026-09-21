/// <reference types="vitest/config" />
import { getViteConfig } from "astro/config";

// Solid's server build doesn't run effects; the state tests need the browser
// build. Vitest resolves through Vite's SSR environment, so set it there, and
// let Vite (not Node) load solid-js so the conditions apply.
const conditions = ["browser", "development"];

export default getViteConfig({
  resolve: { conditions },
  ssr: { resolve: { conditions, externalConditions: conditions } },
  test: {
    include: ["test/**/*.test.ts"],
    server: { deps: { inline: [/solid-js/] } },
  },
});
