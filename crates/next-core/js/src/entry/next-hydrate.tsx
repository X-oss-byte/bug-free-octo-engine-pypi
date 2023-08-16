import "@vercel/turbopack-next/internal/shims-client";

import { initialize, hydrate } from "next/dist/client";
import { initializeHMR } from "@vercel/turbopack-next/dev/client";

import * as _app from "@vercel/turbopack-next/pages/_app";
import * as page from ".";

(async () => {
  console.debug("Initializing Next.js");

  const { assetPrefix } = await initialize({
    webpackHMR: {
      // Expected when `process.env.NODE_ENV === 'development'`
      onUnrecoverableError() {},
    },
  });

  initializeHMR({
    assetPrefix,
  });

  window.__NEXT_P.push(["/_app", () => _app]);
  window.__NEXT_P.push([window.__NEXT_DATA__.page, () => page]);

  console.debug("Hydrating the page");

  await hydrate({});

  console.debug("The page has been hydrated");
})().catch((err) => console.error(err));
