// Provided by the rust generate code
type FileType =
  | "layout"
  | "template"
  | "error"
  | "loading"
  | "not-found"
  | "head";
declare global {
  // an array of all layouts and the page
  const LAYOUT_INFO: ({
    segment: string;
    page?: { module: any; chunks: string[] };
  } & {
    [componentKey in FileType]?: { module: any; chunks: string[] };
  })[];
  // array of chunks for the bootstrap script
  const BOOTSTRAP: string[];
  const IPC: Ipc<unknown, unknown>;
}

import type { Ipc } from "@vercel/turbopack-next/ipc/index";
import type { IncomingMessage, ServerResponse } from "node:http";
import type {
  FlightCSSManifest,
  FlightManifest,
} from "next/dist/build/webpack/plugins/flight-manifest-plugin";
import type { RenderData } from "types/turbopack";

import "next/dist/server/node-polyfill-fetch";
import "next/dist/server/node-polyfill-web-streams";
import "@vercel/turbopack-next/polyfill/async-local-storage";
import { RenderOpts, renderToHTMLOrFlight } from "next/dist/server/app-render";
import { PassThrough } from "stream";
import { ServerResponseShim } from "@vercel/turbopack-next/internal/http";
import { headersFromEntries } from "@vercel/turbopack-next/internal/headers";
import { parse, ParsedUrlQuery } from "node:querystring";

globalThis.__next_require__ = (data) => {
  const [, , , ssr_id] = JSON.parse(data);
  return __turbopack_require__(ssr_id);
};
globalThis.__next_chunk_load__ = () => Promise.resolve();

process.env.__NEXT_NEW_LINK_BEHAVIOR = "true";

const ipc = IPC as Ipc<IpcIncomingMessage, IpcOutgoingMessage>;

type IpcIncomingMessage = {
  type: "headers";
  data: RenderData;
};

type IpcOutgoingMessage = {
  type: "response";
  statusCode: number;
  headers: Array<[string, string]>;
  body: string;
};

const MIME_TEXT_HTML_UTF8 = "text/html; charset=utf-8";

(async () => {
  while (true) {
    const msg = await ipc.recv();

    let renderData: RenderData;
    switch (msg.type) {
      case "headers": {
        renderData = msg.data;
        break;
      }
      default: {
        console.error("unexpected message type", msg.type);
        process.exit(1);
      }
    }

    const result = await runOperation(renderData);

    if (result == null) {
      throw new Error("no html returned");
    }

    ipc.send({
      type: "response",
      statusCode: 200,
      ...result,
    });
  }
})().catch((err) => {
  ipc.sendError(err);
});

// TODO expose these types in next.js
type ComponentModule = () => any;
type ModuleReference = [componentModule: ComponentModule, filePath: string];
export type ComponentsType = {
  [componentKey in FileType]?: ModuleReference;
} & {
  page?: ModuleReference;
};
type LoaderTree = [
  segment: string,
  parallelRoutes: { [parallelRouterKey: string]: LoaderTree },
  components: ComponentsType
];

type ServerComponentsManifest = {
  [id: string]: ServerComponentsManifestModule;
};
type ServerComponentsManifestModule = {
  [exportName: string]: { id: string; chunks: string[]; name: string };
};

async function runOperation(renderData: RenderData) {
  const layoutInfoChunks: Record<string, string[]> = {};
  const pageItem = LAYOUT_INFO[LAYOUT_INFO.length - 1];
  const pageModule = pageItem.page!.module;
  let tree: LoaderTree = [
    "",
    {},
    { page: [() => pageModule.module, "page.js"] },
  ];
  layoutInfoChunks["page"] = pageItem.page!.chunks;
  for (let i = LAYOUT_INFO.length - 2; i >= 0; i--) {
    const info = LAYOUT_INFO[i];
    const components: ComponentsType = {};
    for (const key of Object.keys(info)) {
      if (key === "segment") {
        continue;
      }
      const k = key as FileType;
      components[k] = [() => info[k]!.module.module, `${k}${i}.js`];
      layoutInfoChunks[`${k}${i}`] = info[k]!.chunks;
    }
    tree = [info.segment, { children: tree }, components];
  }

  const proxyMethodsForModule = (
    id: string
  ): ProxyHandler<FlightManifest["__ssr_module_mapping__"][""]> => ({
    get(_target, name) {
      return {
        id,
        chunks: JSON.parse(id)[1],
        name,
      };
    },
  });
  const proxyMethodsNested = (): ProxyHandler<
    FlightManifest["__ssr_module_mapping__"]
  > => {
    return {
      get(target, name, receiver) {
        if (name === "__ssr_module_mapping__") {
          return manifest;
        }
        if (name === "__entry_css_files__") {
          return __entry_css_files__;
        }
        return new Proxy({}, proxyMethodsForModule(name as string));
      },
    };
  };
  const proxyMethods = (): ProxyHandler<FlightManifest> => {
    return {
      get(_target, key: string) {
        if (key === "__ssr_module_mapping__") {
          return new Proxy({} as any, proxyMethodsNested());
        }
        if (key === "__entry_css_files__") {
          return __entry_css_files__;
        }

        // The key is a `${file}#${name}`, but `file` can contain `#` itself.
        // There are 2 possibilities:
        //   "file#"    => id = "file", name = ""
        //   "file#foo" => id = "file", name = "foo"
        const pos = key.lastIndexOf("#");
        let id = key;
        let name = "";
        if (pos === -1) {
          throw new Error("key need to be in format of ${file}#${name}");
        } else {
          id = key.slice(0, pos);
          name = key.slice(pos + 1);
        }

        return {
          id,
          name,
          chunks: JSON.parse(id)[1],
        };
      },
    };
  };
  const manifest: FlightManifest = new Proxy({} as any, proxyMethods());
  const serverCSSManifest: FlightCSSManifest = {};
  const __entry_css_files__: FlightManifest["__entry_css_files__"] = {};
  for (const [key, chunks] of Object.entries(layoutInfoChunks)) {
    const cssChunks = chunks.filter((path) => path.endsWith(".css"));
    serverCSSManifest[`${key}.js`] = cssChunks.map((chunk) =>
      JSON.stringify([chunk, [chunk]])
    );
    __entry_css_files__[key] = cssChunks;
  }
  serverCSSManifest.__entry_css_mods__ = {
    page: serverCSSManifest["page.js"],
  };
  const req: IncomingMessage = {
    url: renderData.url,
    method: renderData.method,
    headers: headersFromEntries(renderData.rawHeaders),
  } as any;
  const res: ServerResponse = new ServerResponseShim(req) as any;
  const parsedQuery = parse(renderData.rawQuery);
  const query = { ...parsedQuery, ...renderData.params };
  const renderOpt: Omit<
    RenderOpts,
    "App" | "Document" | "Component" | "pathname"
  > & { params: ParsedUrlQuery } = {
    params: renderData.params,
    supportsDynamicHTML: true,
    dev: true,
    buildManifest: {
      polyfillFiles: [],
      rootMainFiles: Object.values(layoutInfoChunks)
        .flat()
        .concat(BOOTSTRAP)
        .filter((path) => path.endsWith(".js")),
      devFiles: [],
      ampDevFiles: [],
      lowPriorityFiles: [],
      pages: {
        "/_app": [],
      },
      ampFirstPages: [],
    },
    ComponentMod: {
      ...pageModule,
      default: undefined,
      tree,
      pages: ["page.js"],
    },
    serverComponentManifest: manifest,
    serverCSSManifest,
    runtime: "nodejs",
    serverComponents: true,
    assetPrefix: "",
    pageConfig: pageModule.config,
    reactLoadableManifest: {},
  };
  const result = await renderToHTMLOrFlight(
    req,
    res,
    renderData.path,
    query,
    renderOpt as any as RenderOpts
  );

  if (!result) throw new Error("rendering was not successful");

  let body;
  if (result.isDynamic()) {
    const stream = new PassThrough();
    result.pipe(stream);

    const chunks = [];
    for await (const chunk of stream) {
      chunks.push(chunk);
    }
    body = Buffer.concat(chunks).toString();
  } else {
    body = result.toUnchunkedString();
  }
  return {
    headers: [
      ["Content-Type", result.contentType() ?? MIME_TEXT_HTML_UTF8],
    ] as [string, string][],
    body,
  };
}

// This utility is based on https://github.com/zertosh/htmlescape
// License: https://github.com/zertosh/htmlescape/blob/0527ca7156a524d256101bb310a9f970f63078ad/LICENSE

const ESCAPE_LOOKUP = {
  "&": "\\u0026",
  ">": "\\u003e",
  "<": "\\u003c",
  "\u2028": "\\u2028",
  "\u2029": "\\u2029",
};

const ESCAPE_REGEX = /[&><\u2028\u2029]/g;

export function htmlEscapeJsonString(str: string) {
  return str.replace(
    ESCAPE_REGEX,
    (match) => ESCAPE_LOOKUP[match as keyof typeof ESCAPE_LOOKUP]
  );
}
