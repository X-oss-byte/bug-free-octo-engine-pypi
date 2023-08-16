import type {
  ClientMessage,
  EcmascriptChunkUpdate,
  ResourceIdentifier,
  ServerMessage,
} from "@vercel/turbopack-runtime/types/protocol";
import type {
  ChunkPath,
  UpdateCallback,
  TurbopackGlobals,
} from "@vercel/turbopack-runtime/types";

import stripAnsi from "@vercel/turbopack-next/compiled/strip-ansi";

import {
  onBeforeRefresh,
  onBuildOk,
  onRefresh,
  onTurbopackIssues,
} from "../overlay/client";
import { addEventListener, sendMessage } from "./websocket";
import { ModuleId } from "@vercel/turbopack-runtime/types";
import { HmrUpdateEntry } from "@vercel/turbopack-runtime/types/protocol";

declare var globalThis: TurbopackGlobals;

export type ClientOptions = {
  assetPrefix: string;
};

export function connect({ assetPrefix }: ClientOptions) {
  addEventListener((event) => {
    switch (event.type) {
      case "connected":
        handleSocketConnected();
        break;
      case "message":
        const msg: ServerMessage = JSON.parse(event.message.data);
        handleSocketMessage(msg);
        break;
    }
  });

  const queued = globalThis.TURBOPACK_CHUNK_UPDATE_LISTENERS;
  if (queued != null && !Array.isArray(queued)) {
    throw new Error("A separate HMR handler was already registered");
  }
  globalThis.TURBOPACK_CHUNK_UPDATE_LISTENERS = {
    push: ([chunkPath, callback]: [ChunkPath, UpdateCallback]) => {
      subscribeToChunkUpdate(chunkPath, callback);
    },
  };

  if (Array.isArray(queued)) {
    for (const [chunkPath, callback] of queued) {
      subscribeToChunkUpdate(chunkPath, callback);
    }
  }

  subscribeToInitialCssChunksUpdates(assetPrefix);
}

type UpdateCallbackSet = {
  callbacks: Set<UpdateCallback>;
  unsubscribe: () => void;
};

const updateCallbackSets: Map<ResourceKey, UpdateCallbackSet> = new Map();

function sendJSON(message: ClientMessage) {
  sendMessage(JSON.stringify(message));
}

type ResourceKey = string;

function resourceKey(resource: ResourceIdentifier): ResourceKey {
  return JSON.stringify({
    path: resource.path,
    headers: resource.headers || null,
  });
}

function subscribeToUpdates(resource: ResourceIdentifier): () => void {
  sendJSON({
    type: "subscribe",
    ...resource,
  });

  return () => {
    sendJSON({
      type: "unsubscribe",
      ...resource,
    });
  };
}

function handleSocketConnected() {
  for (const key of updateCallbackSets.keys()) {
    subscribeToUpdates(JSON.parse(key));
  }
}

type AggregatedUpdates = {
  added: Record<ModuleId, HmrUpdateEntry>;
  modified: Record<ModuleId, HmrUpdateEntry>;
  deleted: Set<ModuleId>;
};

// we aggregate all updates until the issues are resolved
const chunksWithUpdates: Map<ResourceKey, AggregatedUpdates> = new Map();

function aggregateUpdates(
  msg: ServerMessage,
  hasIssues: boolean
): ServerMessage {
  const key = resourceKey(msg.resource);
  const aggregated = chunksWithUpdates.get(key);

  if (msg.type === "issues" && aggregated == null && hasIssues) {
    // add an empty record to make sure we don't call `onBuildOk`
    chunksWithUpdates.set(key, {
      added: {},
      modified: {},
      deleted: new Set(),
    });
  }

  if (msg.type === "issues" && aggregated != null) {
    if (!hasIssues) {
      chunksWithUpdates.delete(key);
    }

    return {
      ...msg,
      type: "partial",
      instruction: {
        type: "EcmascriptChunkUpdate",
        added: aggregated.added,
        modified: aggregated.modified,
        deleted: Array.from(aggregated.deleted),
      },
    };
  }

  if (msg.type !== "partial") return msg;

  if (aggregated == null) {
    if (hasIssues) {
      chunksWithUpdates.set(key, {
        added: msg.instruction.added,
        modified: msg.instruction.modified,
        deleted: new Set(msg.instruction.deleted),
      });
    }

    return msg;
  }

  for (const [moduleId, entry] of Object.entries(msg.instruction.added)) {
    const removedDeleted = aggregated.deleted.delete(moduleId);
    if (aggregated.modified[moduleId] != null) {
      console.error(
        `impossible state aggregating updates: module "${moduleId}" was added, but previously modified`
      );
      location.reload();
    }

    if (removedDeleted) {
      aggregated.modified[moduleId] = entry;
    } else {
      aggregated.added[moduleId] = entry;
    }
  }

  for (const [moduleId, entry] of Object.entries(msg.instruction.modified)) {
    if (aggregated.added[moduleId] != null) {
      aggregated.added[moduleId] = entry;
    } else {
      aggregated.modified[moduleId] = entry;
    }

    if (aggregated.deleted.has(moduleId)) {
      console.error(
        `impossible state aggregating updates: module "${moduleId}" was modified, but previously deleted`
      );
      location.reload();
    }
  }

  for (const moduleId of msg.instruction.deleted) {
    delete aggregated.added[moduleId];
    delete aggregated.modified[moduleId];
    aggregated.deleted.add(moduleId);
  }

  if (!hasIssues) {
    chunksWithUpdates.delete(key);
  } else {
    chunksWithUpdates.set(key, aggregated);
  }

  return {
    ...msg,
    instruction: {
      type: "EcmascriptChunkUpdate",
      added: aggregated.added,
      modified: aggregated.modified,
      deleted: Array.from(aggregated.deleted),
    },
  };
}

const CRITICAL = ["bug", "error", "fatal"];

function compareByList(list: any[], a: any, b: any) {
  const aI = list.indexOf(a) + 1 || list.length;
  const bI = list.indexOf(b) + 1 || list.length;
  return aI - bI;
}

function handleIssues(msg: ServerMessage): boolean {
  let hasCriticalIssues = false;

  for (const issue of msg.issues) {
    if (CRITICAL.includes(issue.severity)) {
      console.error(stripAnsi(issue.formatted));
      hasCriticalIssues = true;
    }
  }

  if (msg.issues.length > 0) {
    onTurbopackIssues(msg.issues);
  }

  return hasCriticalIssues;
}

const SEVERITY_ORDER = ["bug", "fatal", "error", "warning", "info", "log"];
const CATEGORY_ORDER = [
  "parse",
  "resolve",
  "code generation",
  "rendering",
  "typescript",
  "other",
];

function handleSocketMessage(msg: ServerMessage) {
  msg.issues.sort((a, b) => {
    const first = compareByList(SEVERITY_ORDER, a.severity, b.severity);
    if (first !== 0) return first;
    return compareByList(CATEGORY_ORDER, a.category, b.category);
  });

  const hasIssues = handleIssues(msg);
  const aggregatedMsg = aggregateUpdates(msg, hasIssues);

  if (hasIssues) return;

  const runHooks = chunksWithUpdates.size === 0;

  if (aggregatedMsg.type !== "issues") {
    if (runHooks) onBeforeRefresh();
    triggerUpdate(aggregatedMsg);
    if (runHooks) onRefresh();
  }

  if (runHooks) onBuildOk();

  // This is used by the Next.js integration test suite to notify it when HMR
  // updates have been completed.
  // TODO: Only run this in test environments (gate by `process.env.__NEXT_TEST_MODE`)
  if (globalThis.__NEXT_HMR_CB) {
    globalThis.__NEXT_HMR_CB();
    globalThis.__NEXT_HMR_CB = null;
  }
}

export function subscribeToChunkUpdate(
  chunkPath: ChunkPath,
  callback: UpdateCallback
): () => void {
  return subscribeToUpdate(
    {
      path: chunkPath,
    },
    callback
  );
}

export function subscribeToUpdate(
  resource: ResourceIdentifier,
  callback: UpdateCallback
) {
  const key = resourceKey(resource);
  let callbackSet: UpdateCallbackSet;
  const existingCallbackSet = updateCallbackSets.get(key);
  if (!existingCallbackSet) {
    callbackSet = {
      callbacks: new Set([callback]),
      unsubscribe: subscribeToUpdates(resource),
    };
    updateCallbackSets.set(key, callbackSet);
  } else {
    existingCallbackSet.callbacks.add(callback);
    callbackSet = existingCallbackSet;
  }

  return () => {
    callbackSet.callbacks.delete(callback);

    if (callbackSet.callbacks.size === 0) {
      callbackSet.unsubscribe();
      updateCallbackSets.delete(key);
    }
  };
}

function triggerUpdate(msg: ServerMessage) {
  const key = resourceKey(msg.resource);
  const callbackSet = updateCallbackSets.get(key);
  if (!callbackSet) {
    return;
  }

  try {
    for (const callback of callbackSet.callbacks) {
      callback(msg);
    }
  } catch (err) {
    console.error(
      `An error occurred during the update of resource \`${msg.resource.path}\``,
      err
    );
    location.reload();
  }
}

// Unlike ES chunks, CSS chunks cannot contain the logic to accept updates.
// They must be reloaded here instead.
function subscribeToInitialCssChunksUpdates(assetPrefix: string) {
  const initialCssChunkLinks: NodeListOf<HTMLLinkElement> =
    document.head.querySelectorAll(`link[rel="stylesheet"]`);

  initialCssChunkLinks.forEach((link) => {
    subscribeToCssChunkUpdates(assetPrefix, link);
  });
}

export function subscribeToCssChunkUpdates(
  assetPrefix: string,
  link: HTMLLinkElement
) {
  const cssChunkPrefix = `${assetPrefix}/`;

  const href = link.href;
  if (href == null) {
    return;
  }

  const { pathname, origin } = new URL(href);
  if (origin !== location.origin || !pathname.startsWith(cssChunkPrefix)) {
    return;
  }

  const chunkPath = pathname.slice(cssChunkPrefix.length);
  subscribeToChunkUpdate(chunkPath, (update) => {
    switch (update.type) {
      case "restart": {
        console.info(`Reloading CSS chunk \`${chunkPath}\``);
        link.replaceWith(link);
        break;
      }
      case "partial":
        throw new Error(`partial CSS chunk updates are not supported`);
      default:
        throw new Error(`unknown update type \`${update}\``);
    }
  });
}
