import type { Ipc } from "@vercel/turbopack-next/ipc/index";
import type { IncomingMessage, ServerResponse } from "node:http";
import { Buffer } from "node:buffer";
import { createServer, makeRequest } from "@vercel/turbopack-next/ipc/server";
import { makeResolver } from "next/dist/server/router.js";
import loadConfig from "next/dist/server/config";
import { PHASE_DEVELOPMENT_SERVER } from "next/dist/shared/lib/constants";

import "next/dist/server/node-polyfill-fetch.js";

type RouterRequest = {
  method: string;
  pathname: string;
  // TODO: not passed to request
  rawHeaders: Array<[string, string]>;
  rawQuery: string;
};

type RouteResult =
  | {
      type: "rewrite";
      url: string;
      headers: Record<string, string>;
    }
  | {
      type: "none";
    };

type IpcOutgoingMessage = {
  type: "jsonValue";
  data: string;
};

type MessageData =
  | { type: "middleware-headers"; data: MiddlewareHeadersResponse }
  | { type: "middleware-body"; data: Uint8Array }
  | {
      type: "full-middleware";
      data: { headers: MiddlewareHeadersResponse; body: number[] };
    }
  | {
      type: "rewrite";
      data: RewriteResponse;
    }
  | { type: "none" };

type RewriteResponse = {
  url: string;
  headers: string[];
};

type MiddlewareHeadersResponse = {
  statusCode: number;
  headers: string[];
};

let resolveRouteMemo: Promise<
  (req: IncomingMessage, res: ServerResponse) => Promise<void>
>;

async function getResolveRoute(
  dir: string
): ReturnType<
  typeof import("next/dist/server/lib/route-resolver").makeResolver
> {
  const nextConfig = await loadConfig(
    PHASE_DEVELOPMENT_SERVER,
    process.cwd(),
    undefined,
    undefined,
    true
  );

  return await makeResolver(dir, nextConfig);
}

export default async function route(
  ipc: Ipc<RouterRequest, IpcOutgoingMessage>,
  routerRequest: RouterRequest,
  dir: string
) {
  const [resolveRoute, server] = await Promise.all([
    (resolveRouteMemo ??= getResolveRoute(dir)),
    createServer(),
  ]);

  try {
    const {
      clientRequest,
      clientResponsePromise,
      serverRequest,
      serverResponse,
    } = await makeRequest(
      server,
      routerRequest.method,
      routerRequest.pathname,
      routerRequest.rawQuery,
      routerRequest.rawHeaders
    );

    // Send the clientRequest, so the server parses everything. We can then pass
    // the serverRequest to Next.js to handle.
    clientRequest.end();

    // The route promise must not block us from starting the client response
    // handling, so we cannot await it yet. By making the call, we allow
    // Next.js to start writing to the response whenever it's ready.
    const routePromise = resolveRoute(serverRequest, serverResponse);

    // Now that the Next.js has started processing the route, the
    // clientResponsePromise will resolve once they write data and then we can
    // begin streaming.
    // We again cannot block on the clientResponsePromise, because an error may
    // occur in the routePromise while we're waiting.
    const responsePromise = clientResponsePromise.then((c) =>
      handleClientResponse(ipc, c)
    );

    // Now that both promises are in progress, we await both so that a
    // rejection in either will end the routing.
    const [response] = await Promise.all([responsePromise, routePromise]);

    server.close();
    return response;
  } catch (e) {
    // Server doesn't need to be closed, because the sendError will terminate
    // the process.
    ipc.sendError(e as Error);
  }
}

async function handleClientResponse(
  _ipc: Ipc<RouterRequest, IpcOutgoingMessage>,
  clientResponse: IncomingMessage
): Promise<MessageData> {
  if (clientResponse.headers["x-nextjs-route-result"] === "1") {
    clientResponse.setEncoding("utf8");
    // We're either a redirect or a rewrite
    let buffer = "";
    for await (const chunk of clientResponse) {
      buffer += chunk;
    }

    const data = JSON.parse(buffer) as RouteResult;

    switch (data.type) {
      case "rewrite":
        return {
          type: "rewrite",
          data: {
            url: data.url,
            headers: Object.entries(data.headers).flat(),
          },
        };
      case "none":
        return {
          type: "none",
        };
    }
  }

  const responseHeaders: MiddlewareHeadersResponse = {
    statusCode: clientResponse.statusCode!,
    headers: clientResponse.rawHeaders,
  };

  // TODO: support streaming middleware
  // ipc.send({
  //   type: "jsonValue",
  //   data: JSON.stringify({
  //     type: "middleware-headers",
  //     data: responseHeaders,
  //   }),
  // });
  // ipc.send({
  //   type: "jsonValue",
  //   data: JSON.stringify({
  //     type: "middleware-body",
  //     data: chunk as Buffer,
  //   }),
  // });

  const buffers = [];
  for await (const chunk of clientResponse) {
    buffers.push(chunk as Buffer);
  }
  return {
    type: "full-middleware",
    data: {
      headers: responseHeaders,
      body: Buffer.concat(buffers).toJSON().data,
    },
  };
}
