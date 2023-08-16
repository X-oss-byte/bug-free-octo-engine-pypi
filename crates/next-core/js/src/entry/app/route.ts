// IPC need to be the first import to allow it to catch errors happening during
// the other imports
import startHandler from "@vercel/turbopack-next/internal/api-server-handler";
import { runEdgeFunction } from "@vercel/turbopack-next/internal/edge";

import { join } from "path";

import "next/dist/server/node-polyfill-fetch.js";

import chunkGroup from "ROUTE_CHUNK_GROUP";

import {
  NodeNextRequest,
  NodeNextResponse,
} from "next/dist/server/base-http/node";

startHandler(async ({ request, response, query, params, path }) => {
  const edgeInfo = {
    name: "edge",
    paths: chunkGroup.map((chunk: string) =>
      join(process.cwd(), ".next/server/app", chunk)
    ),
    wasm: [],
    env: [],
    assets: [],
  };
  await runEdgeFunction({
    edgeInfo,
    outputDir: "app",
    req: new NodeNextRequest(request),
    res: new NodeNextResponse(response),
    query,
    params,
    path,
    onWarning(warning) {
      console.warn(warning);
    },
  });
});
