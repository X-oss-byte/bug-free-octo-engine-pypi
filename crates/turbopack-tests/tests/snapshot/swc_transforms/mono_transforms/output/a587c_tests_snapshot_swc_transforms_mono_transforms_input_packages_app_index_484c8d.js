(self.TURBOPACK = self.TURBOPACK || []).push(["output/a587c_tests_snapshot_swc_transforms_mono_transforms_input_packages_app_index_484c8d.js", {

"[project]/crates/turbopack-tests/tests/snapshot/swc_transforms/mono_transforms/input/packages/app/index.js (ecmascript)": (({ r: __turbopack_require__, x: __turbopack_external_require__, i: __turbopack_import__, s: __turbopack_esm__, v: __turbopack_export_value__, c: __turbopack_cache__, l: __turbopack_load__, k: __turbopack_register_chunk_list__, j: __turbopack_cjs__, p: process, g: global, __dirname }) => (() => {

var __TURBOPACK__imported__module__$5b$project$5d2f$crates$2f$turbopack$2d$tests$2f$tests$2f$snapshot$2f$swc_transforms$2f$mono_transforms$2f$input$2f$packages$2f$component$2f$index$2e$js__$28$ecmascript$29__ = __turbopack_import__("[project]/crates/turbopack-tests/tests/snapshot/swc_transforms/mono_transforms/input/packages/component/index.js (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$crates$2f$turbopack$2d$tests$2f$tests$2f$snapshot$2f$swc_transforms$2f$mono_transforms$2f$input$2f$node_modules$2f$third_party_component$2f$index$2e$js__$28$ecmascript$29__ = __turbopack_import__("[project]/crates/turbopack-tests/tests/snapshot/swc_transforms/mono_transforms/input/node_modules/third_party_component/index.js (ecmascript)");
"__TURBOPACK__ecmascript__hoisting__location__";
;
;
console.log(__TURBOPACK__imported__module__$5b$project$5d2f$crates$2f$turbopack$2d$tests$2f$tests$2f$snapshot$2f$swc_transforms$2f$mono_transforms$2f$input$2f$packages$2f$component$2f$index$2e$js__$28$ecmascript$29__["default"], __TURBOPACK__imported__module__$5b$project$5d2f$crates$2f$turbopack$2d$tests$2f$tests$2f$snapshot$2f$swc_transforms$2f$mono_transforms$2f$input$2f$node_modules$2f$third_party_component$2f$index$2e$js__$28$ecmascript$29__["default"]);

})()),
}, ({ loadedChunks, instantiateRuntimeModule, registerChunkList }) => {
    if (!(true && loadedChunks.has("output/63a02_react_jsx-dev-runtime_a01403.js") && loadedChunks.has("output/8562f_snapshot_swc_transforms_mono_transforms_input_packages_component_index_13a243.js") && loadedChunks.has("output/7b7bf_third_party_component_index_c2ffe3.js"))) return true;
    registerChunkList("output/a587c_tests_snapshot_swc_transforms_mono_transforms_input_packages_app_index_e02442.js.chunk-list.json", ["output/63a02_react_jsx-dev-runtime_a01403.js","output/8562f_snapshot_swc_transforms_mono_transforms_input_packages_component_index_13a243.js","output/7b7bf_third_party_component_index_c2ffe3.js"]);
    instantiateRuntimeModule("[project]/crates/turbopack-tests/tests/snapshot/swc_transforms/mono_transforms/input/packages/app/index.js (ecmascript)");
}
]);
(() => {
if (!Array.isArray(globalThis.TURBOPACK)) {
    return;
}
/** @typedef {import('../types/backend').RuntimeBackend} RuntimeBackend */

/** @type {RuntimeBackend} */
const BACKEND = {
  loadChunk(chunkPath) {
    return new Promise((resolve, reject) => {
      if (chunkPath.endsWith(".css")) {
        const link = document.createElement("link");
        link.rel = "stylesheet";
        link.href = `/${chunkPath}`;
        link.onerror = () => {
          reject();
        };
        link.onload = () => {
          // CSS chunks do not register themselves, and as such must be marked as
          // loaded instantly.
          resolve();
        };
        document.body.appendChild(link);
      } else if (chunkPath.endsWith(".js")) {
        const script = document.createElement("script");
        script.src = `/${chunkPath}`;
        // We'll only mark the chunk as loaded once the script has been executed,
        // which happens in `registerChunk`. Hence the absence of `resolve()` in
        // this branch.
        script.onerror = () => {
          reject();
        };
        document.body.appendChild(script);
      } else {
        throw new Error(`can't infer type of chunk from path ${chunkPath}`);
      }
    });
  },

  unloadChunk(chunkPath) {
    if (chunkPath.endsWith(".css")) {
      const links = document.querySelectorAll(`link[href="/${chunkPath}"]`);
      for (const link of Array.from(links)) {
        link.remove();
      }
    } else if (chunkPath.endsWith(".js")) {
      // Unloading a JS chunk would have no effect, as it lives in the JS
      // runtime once evaluated.
      // However, we still want to remove the script tag from the DOM to keep
      // the HTML somewhat consistent from the user's perspective.
      const scripts = document.querySelectorAll(`script[src="/${chunkPath}"]`);
      for (const script of Array.from(scripts)) {
        script.remove();
      }
    } else {
      throw new Error(`can't infer type of chunk from path ${chunkPath}`);
    }
  },

  reloadChunk(chunkPath) {
    return new Promise((resolve, reject) => {
      if (!chunkPath.endsWith(".css")) {
        reject(new Error("The DOM backend can only reload CSS chunks"));
        return;
      }

      const previousLink = document.querySelector(
        `link[href^="/${chunkPath}"]`
      );

      if (previousLink == null) {
        reject(new Error(`No link element found for chunk ${chunkPath}`));
        return;
      }

      const link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = `/${chunkPath}?t=${Date.now()}`;
      link.onerror = () => {
        reject();
      };
      link.onload = () => {
        // First load the new CSS, then remove the old one. This prevents visible
        // flickering that would happen in-between removing the previous CSS and
        // loading the new one.
        previousLink.remove();

        // CSS chunks do not register themselves, and as such must be marked as
        // loaded instantly.
        resolve();
      };

      // Make sure to insert the new CSS right after the previous one, so that
      // its precedence is higher.
      previousLink.parentElement.insertBefore(link, previousLink.nextSibling);
    });
  },

  restart: () => self.location.reload(),
};
/* eslint-disable @next/next/no-assign-module-variable */

/** @typedef {import('../types').ChunkRegistration} ChunkRegistration */
/** @typedef {import('../types').ModuleFactory} ModuleFactory */

/** @typedef {import('../types').ChunkPath} ChunkPath */
/** @typedef {import('../types').ModuleId} ModuleId */
/** @typedef {import('../types').GetFirstModuleChunk} GetFirstModuleChunk */

/** @typedef {import('../types').Module} Module */
/** @typedef {import('../types').SourceInfo} SourceInfo */
/** @typedef {import('../types').SourceType} SourceType */
/** @typedef {import('../types').SourceType.Runtime} SourceTypeRuntime */
/** @typedef {import('../types').SourceType.Parent} SourceTypeParent */
/** @typedef {import('../types').SourceType.Update} SourceTypeUpdate */
/** @typedef {import('../types').Exports} Exports */
/** @typedef {import('../types').EsmInteropNamespace} EsmInteropNamespace */
/** @typedef {import('../types').Runnable} Runnable */

/** @typedef {import('../types').Runtime} Runtime */

/** @typedef {import('../types').RefreshHelpers} RefreshHelpers */
/** @typedef {import('../types/hot').Hot} Hot */
/** @typedef {import('../types/hot').HotData} HotData */
/** @typedef {import('../types/hot').AcceptCallback} AcceptCallback */
/** @typedef {import('../types/hot').AcceptErrorHandler} AcceptErrorHandler */
/** @typedef {import('../types/hot').HotState} HotState */
/** @typedef {import('../types/protocol').PartialUpdate} PartialUpdate */
/** @typedef {import('../types/protocol').ChunkListUpdate} ChunkListUpdate */
/** @typedef {import('../types/protocol').EcmascriptMergedUpdate} EcmascriptMergedUpdate */
/** @typedef {import('../types/protocol').EcmascriptMergedChunkUpdate} EcmascriptMergedChunkUpdate */
/** @typedef {import('../types/protocol').EcmascriptModuleEntry} EcmascriptModuleEntry */

/** @typedef {import('../types/runtime').Loader} Loader */
/** @typedef {import('../types/runtime').ModuleEffect} ModuleEffect */

/** @type {Array<Runnable>} */
let runnable = [];
/** @type {Object.<ModuleId, ModuleFactory>} */
const moduleFactories = { __proto__: null };
/** @type {Object.<ModuleId, Module>} */
const moduleCache = { __proto__: null };
/**
 * Contains the IDs of all chunks that have been loaded.
 *
 * @type {Set<ChunkPath>}
 */
const loadedChunks = new Set();
/**
 * Maps a chunk ID to the chunk's loader if the chunk is currently being loaded.
 *
 * @type {Map<ChunkPath, Loader>}
 */
const chunkLoaders = new Map();
/**
 * Maps module IDs to persisted data between executions of their hot module
 * implementation (`hot.data`).
 *
 * @type {Map<ModuleId, HotData>}
 */
const moduleHotData = new Map();
/**
 * Maps module instances to their hot module state.
 *
 * @type {Map<Module, HotState>}
 */
const moduleHotState = new Map();
/**
 * Module IDs that are instantiated as part of the runtime of a chunk.
 *
 * @type {Set<ModuleId>}
 */
const runtimeModules = new Set();
/**
 * Map from module ID to the chunks that contain this module.
 *
 * In HMR, we need to keep track of which modules are contained in which so
 * chunks. This is so we don't eagerly dispose of a module when it is removed
 * from chunk A, but still exists in chunk B.
 *
 * @type {Map<ModuleId, Set<ChunkPath>>}
 */
const moduleChunksMap = new Map();
/**
 * Map from chunk path to all modules it contains.
 * @type {Map<ModuleId, Set<ChunkPath>>}
 */
const chunkModulesMap = new Map();
/**
 * Chunk lists that contain a runtime. When these chunk lists receive an update
 * that can't be reconciled with the current state of the page, we need to
 * reload the runtime entirely.
 * @type {Set<ChunkPath>}
 */
const runtimeChunkLists = new Set();
/**
 * Map from chunk list to the chunk paths it contains.
 * @type {Map<ChunkPath, Set<ChunkPath>>}
 */
const chunkListChunksMap = new Map();
/**
 * Map from chunk path to the chunk lists it belongs to.
 * @type {Map<ChunkPath, Set<ChunkPath>>}
 */
const chunkChunkListsMap = new Map();

const hOP = Object.prototype.hasOwnProperty;
const _process =
  typeof process !== "undefined"
    ? process
    : {
        env: {},
        // Some modules rely on `process.browser` to execute browser-specific code.
        // NOTE: `process.browser` is specific to Webpack.
        browser: true,
      };

const toStringTag = typeof Symbol !== "undefined" && Symbol.toStringTag;

/**
 * @param {any} obj
 * @param {PropertyKey} name
 * @param {PropertyDescriptor & ThisType<any>} options
 */
function defineProp(obj, name, options) {
  if (!hOP.call(obj, name)) Object.defineProperty(obj, name, options);
}

/**
 * Adds the getters to the exports object
 *
 * @param {Exports} exports
 * @param {Record<string, () => any>} getters
 */
function esm(exports, getters) {
  defineProp(exports, "__esModule", { value: true });
  if (toStringTag) defineProp(exports, toStringTag, { value: "Module" });
  for (const key in getters) {
    defineProp(exports, key, { get: getters[key], enumerable: true });
  }
}

/**
 * Adds the getters to the exports object
 *
 * @param {Exports} exports
 * @param {Record<string, any>} props
 */
function cjs(exports, props) {
  for (const key in props) {
    defineProp(exports, key, { get: () => props[key], enumerable: true });
  }
}

/**
 * @param {Module} module
 * @param {any} value
 */
function exportValue(module, value) {
  module.exports = value;
}

/**
 * @param {Record<string, any>} obj
 * @param {string} key
 */
function createGetter(obj, key) {
  return () => obj[key];
}

/**
 * @param {Exports} raw
 * @param {EsmInteropNamespace} ns
 * @param {boolean} [allowExportDefault]
 */
function interopEsm(raw, ns, allowExportDefault) {
  /** @type {Object.<string, () => any>} */
  const getters = { __proto__: null };
  for (const key in raw) {
    getters[key] = createGetter(raw, key);
  }
  if (!(allowExportDefault && "default" in getters)) {
    getters["default"] = () => raw;
  }
  esm(ns, getters);
}

/**
 * @param {Module} sourceModule
 * @param {ModuleId} id
 * @param {boolean} allowExportDefault
 * @returns {EsmInteropNamespace}
 */
function esmImport(sourceModule, id, allowExportDefault) {
  const module = getOrInstantiateModuleFromParent(id, sourceModule);
  const raw = module.exports;
  if (raw.__esModule) return raw;
  if (module.interopNamespace) return module.interopNamespace;
  const ns = (module.interopNamespace = {});
  interopEsm(raw, ns, allowExportDefault);
  return ns;
}

/**
 * @param {Module} sourceModule
 * @param {ModuleId} id
 * @returns {Exports}
 */
function commonJsRequire(sourceModule, id) {
  return getOrInstantiateModuleFromParent(id, sourceModule).exports;
}

function externalRequire(id, esm) {
  let raw;
  try {
    raw = require(id);
  } catch (err) {
    // TODO(alexkirsz) This can happen when a client-side module tries to load
    // an external module we don't provide a shim for (e.g. querystring, url).
    // For now, we fail semi-silently, but in the future this should be a
    // compilation error.
    throw new Error(`Failed to load external module ${id}: ${err}`);
  }
  if (!esm || raw.__esModule) {
    return raw;
  }
  const ns = {};
  interopEsm(raw, ns, true);
  return ns;
}
externalRequire.resolve = (name, opt) => {
  return require.resolve(name, opt);
};

/**
 * @param {ModuleId} from
 * @param {string} chunkPath
 * @returns {Promise<any> | undefined}
 */
function loadChunk(from, chunkPath) {
  if (loadedChunks.has(chunkPath)) {
    return Promise.resolve();
  }

  const chunkLoader = getOrCreateChunkLoader(chunkPath, from);

  return chunkLoader.promise;
}

/**
 * @param {string} chunkPath
 * @param {ModuleId} from
 * @returns {Loader}
 */
function getOrCreateChunkLoader(chunkPath, from) {
  let chunkLoader = chunkLoaders.get(chunkPath);
  if (chunkLoader) {
    return chunkLoader;
  }

  let resolve;
  let reject;
  const promise = new Promise((innerResolve, innerReject) => {
    resolve = innerResolve;
    reject = innerReject;
  });

  const onError = (error) => {
    chunkLoaders.delete(chunkPath);
    reject(
      new Error(
        `Failed to load chunk from ${chunkPath}${error ? `: ${error}` : ""}`
      )
    );
  };

  const onLoad = () => {
    loadedChunks.add(chunkPath);
    chunkLoaders.delete(chunkPath);
    resolve();
  };

  chunkLoader = {
    promise,
    onLoad,
  };
  chunkLoaders.set(chunkPath, chunkLoader);

  BACKEND.loadChunk(chunkPath, from).then(onLoad, onError);

  return chunkLoader;
}

/** @type {SourceTypeRuntime} */
const SourceTypeRuntime = 0;
/** @type {SourceTypeParent} */
const SourceTypeParent = 1;
/** @type {SourceTypeUpdate} */
const SourceTypeUpdate = 2;

/**
 *
 * @param {ModuleId} id
 * @param {SourceInfo} source
 * @returns {Module}
 */
function instantiateModule(id, source) {
  const moduleFactory = moduleFactories[id];
  if (typeof moduleFactory !== "function") {
    // This can happen if modules incorrectly handle HMR disposes/updates,
    // e.g. when they keep a `setTimeout` around which still executes old code
    // and contains e.g. a `require("something")` call.
    let instantiationReason;
    switch (source.type) {
      case SourceTypeRuntime:
        instantiationReason = "as a runtime entry";
        break;
      case SourceTypeParent:
        instantiationReason = `because it was required from module ${source.parentId}`;
        break;
      case SourceTypeUpdate:
        instantiationReason = "because of an HMR update";
        break;
    }
    throw new Error(
      `Module ${id} was instantiated ${instantiationReason}, but the module factory is not available. It might have been deleted in an HMR update.`
    );
  }

  const hotData = moduleHotData.get(id);
  const { hot, hotState } = createModuleHot(hotData);

  /** @type {Module} */
  const module = {
    exports: {},
    loaded: false,
    id,
    parents: undefined,
    children: [],
    interopNamespace: undefined,
    hot,
  };
  moduleCache[id] = module;
  moduleHotState.set(module, hotState);

  switch (source.type) {
    case SourceTypeRuntime:
      runtimeModules.add(id);
      module.parents = [];
      break;
    case SourceTypeParent:
      // No need to add this module as a child of the parent module here, this
      // has already been taken care of in `getOrInstantiateModuleFromParent`.
      module.parents = [source.parentId];
      break;
    case SourceTypeUpdate:
      module.parents = source.parents || [];
      break;
  }

  runModuleExecutionHooks(module, () => {
    moduleFactory.call(module.exports, {
      e: module.exports,
      r: commonJsRequire.bind(null, module),
      x: externalRequire,
      i: esmImport.bind(null, module),
      s: esm.bind(null, module.exports),
      j: cjs.bind(null, module.exports),
      v: exportValue.bind(null, module),
      m: module,
      c: moduleCache,
      l: loadChunk.bind(null, id),
      k: registerChunkList,
      p: _process,
      g: globalThis,
      __dirname: module.id.replace(/(^|\/)[\/]+$/, ""),
    });
  });

  module.loaded = true;
  if (module.interopNamespace) {
    // in case of a circular dependency: cjs1 -> esm2 -> cjs1
    interopEsm(module.exports, module.interopNamespace);
  }

  return module;
}

/**
 * NOTE(alexkirsz) Webpack has an "module execution" interception hook that
 * Next.js' React Refresh runtime hooks into to add module context to the
 * refresh registry.
 *
 * @param {Module} module
 * @param {() => void} executeModule
 */
function runModuleExecutionHooks(module, executeModule) {
  const cleanupReactRefreshIntercept =
    typeof globalThis.$RefreshInterceptModuleExecution$ === "function"
      ? globalThis.$RefreshInterceptModuleExecution$(module.id)
      : () => {};

  executeModule();

  if ("$RefreshHelpers$" in globalThis) {
    // This pattern can also be used to register the exports of
    // a module with the React Refresh runtime.
    registerExportsAndSetupBoundaryForReactRefresh(
      module,
      globalThis.$RefreshHelpers$
    );
  }

  cleanupReactRefreshIntercept();
}

/**
 * Retrieves a module from the cache, or instantiate it if it is not cached.
 *
 * @param {ModuleId} id
 * @param {Module} sourceModule
 * @returns {Module}
 */
function getOrInstantiateModuleFromParent(id, sourceModule) {
  if (!sourceModule.hot.active) {
    console.warn(
      `Unexpected import of module ${id} from module ${sourceModule.id}, which was deleted by an HMR update`
    );
  }

  const module = moduleCache[id];

  if (sourceModule.children.indexOf(id) === -1) {
    sourceModule.children.push(id);
  }

  if (module) {
    if (module.parents.indexOf(sourceModule.id) === -1) {
      module.parents.push(sourceModule.id);
    }

    return module;
  }

  return instantiateModule(id, {
    type: SourceTypeParent,
    parentId: sourceModule.id,
  });
}

/**
 * This is adapted from https://github.com/vercel/next.js/blob/3466862d9dc9c8bb3131712134d38757b918d1c0/packages/react-refresh-utils/internal/ReactRefreshModule.runtime.ts
 *
 * @param {Module} module
 * @param {RefreshHelpers} helpers
 */
function registerExportsAndSetupBoundaryForReactRefresh(module, helpers) {
  const currentExports = module.exports;
  const prevExports = module.hot.data.prevExports ?? null;

  helpers.registerExportsForReactRefresh(currentExports, module.id);

  // A module can be accepted automatically based on its exports, e.g. when
  // it is a Refresh Boundary.
  if (helpers.isReactRefreshBoundary(currentExports)) {
    // Save the previous exports on update so we can compare the boundary
    // signatures.
    module.hot.dispose((data) => {
      data.prevExports = currentExports;
    });
    // Unconditionally accept an update to this module, we'll check if it's
    // still a Refresh Boundary later.
    module.hot.accept();

    // This field is set when the previous version of this module was a
    // Refresh Boundary, letting us know we need to check for invalidation or
    // enqueue an update.
    if (prevExports !== null) {
      // A boundary can become ineligible if its exports are incompatible
      // with the previous exports.
      //
      // For example, if you add/remove/change exports, we'll want to
      // re-execute the importing modules, and force those components to
      // re-render. Similarly, if you convert a class component to a
      // function, we want to invalidate the boundary.
      if (
        helpers.shouldInvalidateReactRefreshBoundary(
          prevExports,
          currentExports
        )
      ) {
        module.hot.invalidate();
      } else {
        helpers.scheduleUpdate();
      }
    }
  } else {
    // Since we just executed the code for the module, it's possible that the
    // new exports made it ineligible for being a boundary.
    // We only care about the case when we were _previously_ a boundary,
    // because we already accepted this update (accidental side effect).
    const isNoLongerABoundary = prevExports !== null;
    if (isNoLongerABoundary) {
      module.hot.invalidate();
    }
  }
}

/**
 * @param {ModuleId[]} dependencyChain
 * @returns {string}
 */
function formatDependencyChain(dependencyChain) {
  return `Dependency chain: ${dependencyChain.join(" -> ")}`;
}

/**
 * @param {EcmascriptModuleEntry} entry
 * @returns {ModuleFactory}
 * @private
 */
function _eval({ code, url, map }) {
  code += `\n\n//# sourceURL=${location.origin}${url}`;
  if (map) code += `\n//# sourceMappingURL=${map}`;
  return eval(code);
}

/**
 * @param {Map<ModuleId, EcmascriptModuleEntry>} added
 * @param {Map<ModuleId, EcmascriptModuleEntry>} modified
 * @param {Record<ModuleId, EcmascriptModuleEntry>} code
 * @returns {{outdatedModules: Set<any>, newModuleFactories: Map<any, any>}}
 */
function computeOutdatedModules(added, modified, code) {
  const outdatedModules = new Set();
  const newModuleFactories = new Map();

  for (const [moduleId, entry] of added) {
    newModuleFactories.set(moduleId, _eval(entry));
  }

  for (const [moduleId, entry] of modified) {
    const effect = getAffectedModuleEffects(moduleId);

    switch (effect.type) {
      case "unaccepted":
        throw new Error(
          `cannot apply update: unaccepted module. ${formatDependencyChain(
            effect.dependencyChain
          )}.`
        );
      case "self-declined":
        throw new Error(
          `cannot apply update: self-declined module. ${formatDependencyChain(
            effect.dependencyChain
          )}.`
        );
      case "accepted":
        newModuleFactories.set(moduleId, _eval(entry));
        for (const outdatedModuleId of effect.outdatedModules) {
          outdatedModules.add(outdatedModuleId);
        }
        break;
      // TODO(alexkirsz) Dependencies: handle dependencies effects.
    }
  }

  return { outdatedModules, newModuleFactories };
}

/**
 * @param {Iterable<ModuleId>} outdatedModules
 * @returns {{ moduleId: ModuleId, errorHandler: true | Function }[]}
 */
function computeOutdatedSelfAcceptedModules(outdatedModules) {
  const outdatedSelfAcceptedModules = [];
  for (const moduleId of outdatedModules) {
    const module = moduleCache[moduleId];
    const hotState = moduleHotState.get(module);
    if (module && hotState.selfAccepted && !hotState.selfInvalidated) {
      outdatedSelfAcceptedModules.push({
        moduleId,
        errorHandler: hotState.selfAccepted,
      });
    }
  }
  return outdatedSelfAcceptedModules;
}

/**
 * Adds, deletes, and moves modules between chunks. This must happen before the
 * dispose phase as it needs to know which modules were removed from all chunks,
 * which we can only compute *after* taking care of added and moved modules.
 *
 * @param {Map<ChunkPath, Set<ModuleId>>} chunksAddedModules
 * @param {Map<ChunkPath, Set<ModuleId>>} chunksDeletedModules
 * @returns {{ disposedModules: Set<ModuleId> }}
 */
function updateChunksPhase(chunksAddedModules, chunksDeletedModules) {
  for (const [chunkPath, addedModuleIds] of chunksAddedModules) {
    for (const moduleId of addedModuleIds) {
      addModuleToChunk(moduleId, chunkPath);
    }
  }

  const disposedModules = new Set();
  for (const [chunkPath, addedModuleIds] of chunksDeletedModules) {
    for (const moduleId of addedModuleIds) {
      if (removeModuleFromChunk(moduleId, chunkPath)) {
        disposedModules.add(moduleId);
      }
    }
  }

  return { disposedModules };
}

/**
 * @param {Iterable<ModuleId>} outdatedModules
 * @param {Set<ModuleId>} disposedModules
 * @return {{ outdatedModuleParents: Map<ModuleId, Array<ModuleId>> }}
 */
function disposePhase(outdatedModules, disposedModules) {
  for (const moduleId of outdatedModules) {
    disposeModule(moduleId, "replace");
  }

  for (const moduleId of disposedModules) {
    disposeModule(moduleId, "clear");
  }

  // Removing modules from the module cache is a separate step.
  // We also want to keep track of previous parents of the outdated modules.
  const outdatedModuleParents = new Map();
  for (const moduleId of outdatedModules) {
    const oldModule = moduleCache[moduleId];
    outdatedModuleParents.set(moduleId, oldModule?.parents);
    delete moduleCache[moduleId];
  }

  // TODO(alexkirsz) Dependencies: remove outdated dependency from module
  // children.

  return { outdatedModuleParents };
}

/**
 * Disposes of an instance of a module.
 *
 * Returns the persistent hot data that should be kept for the next module
 * instance.
 *
 * NOTE: mode = "replace" will not remove modules from the moduleCache.
 * This must be done in a separate step afterwards.
 * This is important because all modules need to be diposed to update the
 * parent/child relationships before they are actually removed from the moduleCache.
 * If this would be done in this method, following disposeModulecalls won't find
 * the module from the module id in the cache.
 *
 * @param {ModuleId} moduleId
 * @param {"clear" | "replace"} mode
 */
function disposeModule(moduleId, mode) {
  const module = moduleCache[moduleId];
  if (!module) {
    return;
  }

  const hotState = moduleHotState.get(module);
  const data = {};

  // Run the `hot.dispose` handler, if any, passing in the persistent
  // `hot.data` object.
  for (const disposeHandler of hotState.disposeHandlers) {
    disposeHandler(data);
  }

  // This used to warn in `getOrInstantiateModuleFromParent` when a disposed
  // module is still importing other modules.
  module.hot.active = false;

  moduleHotState.delete(module);

  // TODO(alexkirsz) Dependencies: delete the module from outdated deps.

  // Remove the disposed module from its children's parents list.
  // It will be added back once the module re-instantiates and imports its
  // children again.
  for (const childId of module.children) {
    const child = moduleCache[childId];
    if (!child) {
      continue;
    }

    const idx = child.parents.indexOf(module.id);
    if (idx >= 0) {
      child.parents.splice(idx, 1);
    }
  }

  switch (mode) {
    case "clear":
      delete moduleCache[module.id];
      moduleHotData.delete(module.id);
      break;
    case "replace":
      moduleHotData.set(module.id, data);
      break;
    default:
      invariant(mode, (mode) => `invalid mode: ${mode}`);
  }
}

/**
 *
 * @param {{ moduleId: ModuleId, errorHandler: true | Function }[]} outdatedSelfAcceptedModules
 * @param {Map<ModuleId, ModuleFactory>} newModuleFactories
 * @param {Map<ModuleId, Array<ModuleId>>} outdatedModuleParents
 */
function applyPhase(
  outdatedSelfAcceptedModules,
  newModuleFactories,
  outdatedModuleParents
) {
  // Update module factories.
  for (const [moduleId, factory] of newModuleFactories.entries()) {
    moduleFactories[moduleId] = factory;
  }

  // TODO(alexkirsz) Run new runtime entries here.

  // TODO(alexkirsz) Dependencies: call accept handlers for outdated deps.

  // Re-instantiate all outdated self-accepted modules.
  for (const { moduleId, errorHandler } of outdatedSelfAcceptedModules) {
    try {
      instantiateModule(moduleId, {
        type: SourceTypeUpdate,
        parents: outdatedModuleParents.get(moduleId),
      });
    } catch (err) {
      if (typeof errorHandler === "function") {
        try {
          errorHandler(err, { moduleId, module: moduleCache[moduleId] });
        } catch (_) {
          // Ignore error.
        }
      }
    }
  }
}

/**
 * Utility function to ensure all variants of an enum are handled.
 * @param {never} never
 * @param {(arg: any) => string} computeMessage
 * @returns {never}
 */
function invariant(never, computeMessage) {
  throw new Error(`Invariant: ${computeMessage(never)}`);
}

/**
 *
 * @param {ChunkPath} chunkListPath
 * @param {PartialUpdate} update
 */
function applyUpdate(chunkListPath, update) {
  switch (update.type) {
    case "ChunkListUpdate":
      applyChunkListUpdate(chunkListPath, update);
      break;
    default:
      invariant(update, (update) => `Unknown update type: ${update.type}`);
  }
}

/**
 *
 * @param {ChunkPath} chunkListPath
 * @param {ChunkListUpdate} update
 */
function applyChunkListUpdate(chunkListPath, update) {
  if (update.merged != null) {
    for (const merged of update.merged) {
      switch (merged.type) {
        case "EcmascriptMergedUpdate":
          applyEcmascriptMergedUpdate(chunkListPath, merged);
          break;
        default:
          invariant(merged, (merged) => `Unknown merged type: ${merged.type}`);
      }
    }
  }

  if (update.chunks != null) {
    for (const [chunkPath, chunkUpdate] of Object.entries(update.chunks)) {
      switch (chunkUpdate.type) {
        case "added":
          BACKEND.loadChunk(chunkPath);
          break;
        case "total":
          BACKEND.reloadChunk?.(chunkPath);
          break;
        case "deleted":
          loadedChunks.delete(chunkPath);
          BACKEND.unloadChunk?.(chunkPath);
          break;
        case "partial":
          invariant(
            chunkUpdate.instruction,
            (instruction) =>
              `Unknown partial instruction: ${JSON.stringify(instruction)}.`
          );
        default:
          invariant(
            chunkUpdate,
            (chunkUpdate) => `Unknown chunk update type: ${chunkUpdate.type}`
          );
      }
    }
  }
}

/**
 * @param {ChunkPath} chunkPath
 * @param {EcmascriptMergedUpdate} update
 */
function applyEcmascriptMergedUpdate(chunkPath, update) {
  const { entries = {}, chunks = {} } = update;
  const { added, modified, deleted, chunksAdded, chunksDeleted } =
    computeChangedModules(entries, chunks);
  const { outdatedModules, newModuleFactories } = computeOutdatedModules(
    added,
    modified,
    entries
  );
  const outdatedSelfAcceptedModules =
    computeOutdatedSelfAcceptedModules(outdatedModules);
  const { disposedModules } = updateChunksPhase(chunksAdded, chunksDeleted);
  const { outdatedModuleParents } = disposePhase(
    outdatedModules,
    disposedModules
  );
  applyPhase(
    outdatedSelfAcceptedModules,
    newModuleFactories,
    outdatedModuleParents
  );
}

/**
 * @param {Record<ModuleId, EcmascriptModuleEntry>} entries
 * @param {Record<ChunkPath, EcmascriptMergedChunkUpdate>} updates
 * @returns {{
 *  added: Map<ModuleId, EcmascriptModuleEntry | undefined>,
 *  modified: Map<ModuleId, EcmascriptModuleEntry>,
 *  deleted: Set<ModuleId>,
 *  chunksAdded: Map<ChunkPath, Set<ModuleId>>,
 *  chunksDeleted: Map<ChunkPath, Set<ModuleId>>,
 * }}
 */
function computeChangedModules(entries, updates) {
  const chunksAdded = new Map();
  const chunksDeleted = new Map();
  const added = new Map();
  const modified = new Map();
  const deleted = new Set();

  for (const [chunkPath, mergedChunkUpdate] of Object.entries(updates)) {
    switch (mergedChunkUpdate.type) {
      case "added": {
        const updateAdded = new Set(mergedChunkUpdate.modules);
        for (const moduleId of updateAdded) {
          added.set(moduleId, entries[moduleId]);
        }
        chunksAdded.set(chunkPath, updateAdded);
        break;
      }
      case "deleted": {
        // We could also use `mergedChunkUpdate.modules` here.
        const updateDeleted = new Set(chunkModulesMap.get(chunkPath));
        for (const moduleId of updateDeleted) {
          deleted.add(moduleId);
        }
        chunksDeleted.set(chunkPath, updateDeleted);
        break;
      }
      case "partial": {
        const updateAdded = new Set(mergedChunkUpdate.added);
        const updateDeleted = new Set(mergedChunkUpdate.deleted);
        for (const moduleId of updateAdded) {
          added.set(moduleId, entries[moduleId]);
        }
        for (const moduleId of updateDeleted) {
          deleted.add([moduleId, chunkPath]);
        }
        chunksAdded.set(chunkPath, updateAdded);
        chunksDeleted.set(chunkPath, updateDeleted);
        break;
      }
      default:
        invariant(
          mergedChunkUpdate,
          (mergedChunkUpdate) =>
            `Unknown merged chunk update type: ${mergedChunkUpdate.type}`
        );
    }
  }

  // If a module was added from one chunk and deleted from another in the same update,
  // consider it to be modified, as it means the module was moved from one chunk to another
  // AND has new code in a single update.
  for (const moduleId of added.keys()) {
    if (deleted.has(moduleId)) {
      added.delete(moduleId);
      deleted.delete(moduleId);
    }
  }

  for (const [moduleId, entry] of Object.entries(entries)) {
    // Modules that haven't been added to any chunk but have new code are considered
    // to be modified.
    // This needs to be under the previous loop, as we need it to get rid of modules
    // that were added and deleted in the same update.
    if (!added.has(moduleId)) {
      modified.set(moduleId, entry);
    }
  }

  return { added, deleted, modified, chunksAdded, chunksDeleted };
}

/**
 *
 * @param {ModuleId} moduleId
 * @returns {ModuleEffect}
 */
function getAffectedModuleEffects(moduleId) {
  const outdatedModules = new Set();

  /** @typedef {{moduleId?: ModuleId, dependencyChain: ModuleId[]}} QueueItem */

  /** @type {QueueItem[]} */
  const queue = [
    {
      moduleId,
      dependencyChain: [],
    },
  ];

  while (queue.length > 0) {
    const { moduleId, dependencyChain } =
      /** @type {QueueItem} */ queue.shift();
    outdatedModules.add(moduleId);

    // We've arrived at the runtime of the chunk, which means that nothing
    // else above can accept this update.
    if (moduleId === undefined) {
      return {
        type: "unaccepted",
        dependencyChain,
      };
    }

    const module = moduleCache[moduleId];
    const hotState = moduleHotState.get(module);

    if (
      // The module is not in the cache. Since this is a "modified" update,
      // it means that the module was never instantiated before.
      !module || // The module accepted itself without invalidating globalThis.
      // TODO is that right?
      (hotState.selfAccepted && !hotState.selfInvalidated)
    ) {
      continue;
    }

    if (hotState.selfDeclined) {
      return {
        type: "self-declined",
        dependencyChain,
        moduleId,
      };
    }

    if (runtimeModules.has(moduleId)) {
      queue.push({
        moduleId: undefined,
        dependencyChain: [...dependencyChain, moduleId],
      });
      continue;
    }

    for (const parentId of module.parents) {
      const parent = moduleCache[parentId];

      if (!parent) {
        // TODO(alexkirsz) Is this even possible?
        continue;
      }

      // TODO(alexkirsz) Dependencies: check accepted and declined
      // dependencies here.

      queue.push({
        moduleId: parentId,
        dependencyChain: [...dependencyChain, moduleId],
      });
    }
  }

  return {
    type: "accepted",
    moduleId,
    outdatedModules,
  };
}

/**
 * @param {ChunkPath} chunkListPath
 * @param {import('../types/protocol').ServerMessage} update
 */
function handleApply(chunkListPath, update) {
  switch (update.type) {
    case "partial": {
      // This indicates that the update is can be applied to the current state of the application.
      applyUpdate(chunkListPath, update.instruction);
      break;
    }
    case "restart": {
      // This indicates that there is no way to apply the update to the
      // current state of the application, and that the application must be
      // restarted.
      BACKEND.restart();
      break;
    }
    case "notFound": {
      // This indicates that the chunk list no longer exists: either the dynamic import which created it was removed,
      // or the page itself was deleted.
      // If it is a dynamic import, we simply discard all modules that the chunk has exclusive access to.
      // If it is a runtime chunk list, we restart the application.
      if (runtimeChunkLists.has(chunkListPath)) {
        BACKEND.restart();
      } else {
        disposeChunkList(chunkListPath);
      }
      break;
    }
    default:
      throw new Error(`Unknown update type: ${update.type}`);
  }
}

/**
 * @param {HotData} [hotData]
 * @returns {{hotState: HotState, hot: Hot}}
 */
function createModuleHot(hotData) {
  /** @type {HotState} */
  const hotState = {
    selfAccepted: false,
    selfDeclined: false,
    selfInvalidated: false,
    disposeHandlers: [],
  };

  /**
   * TODO(alexkirsz) Support full (dep, callback, errorHandler) form.
   *
   * @param {string | string[] | AcceptErrorHandler} [dep]
   * @param {AcceptCallback} [_callback]
   * @param {AcceptErrorHandler} [_errorHandler]
   */
  function accept(dep, _callback, _errorHandler) {
    if (dep === undefined) {
      hotState.selfAccepted = true;
    } else if (typeof dep === "function") {
      hotState.selfAccepted = dep;
    } else {
      throw new Error("unsupported `accept` signature");
    }
  }

  /** @type {Hot} */
  const hot = {
    // TODO(alexkirsz) This is not defined in the HMR API. It was used to
    // decide whether to warn whenever an HMR-disposed module required other
    // modules. We might want to remove it.
    active: true,

    data: hotData ?? {},

    accept: accept,

    decline: (dep) => {
      if (dep === undefined) {
        hotState.selfDeclined = true;
      } else {
        throw new Error("unsupported `decline` signature");
      }
    },

    dispose: (callback) => {
      hotState.disposeHandlers.push(callback);
    },

    addDisposeHandler: (callback) => {
      hotState.disposeHandlers.push(callback);
    },

    removeDisposeHandler: (callback) => {
      const idx = hotState.disposeHandlers.indexOf(callback);
      if (idx >= 0) {
        hotState.disposeHandlers.splice(idx, 1);
      }
    },

    invalidate: () => {
      hotState.selfInvalidated = true;
      // TODO(alexkirsz) The original HMR code had management-related code
      // here.
    },

    // NOTE(alexkirsz) This is part of the management API, which we don't
    // implement, but the Next.js React Refresh runtime uses this to decide
    // whether to schedule an update.
    status: () => "idle",

    // NOTE(alexkirsz) Since we always return "idle" for now, these are no-ops.
    addStatusHandler: (_handler) => {},
    removeStatusHandler: (_handler) => {},
  };

  return { hot, hotState };
}

/**
 * Adds a module to a chunk.
 *
 * @param {ModuleId} moduleId
 * @param {ChunkPath} chunkPath
 */
function addModuleToChunk(moduleId, chunkPath) {
  let moduleChunks = moduleChunksMap.get(moduleId);
  if (!moduleChunks) {
    moduleChunks = new Set([chunkPath]);
    moduleChunksMap.set(moduleId, moduleChunks);
  } else {
    moduleChunks.add(chunkPath);
  }

  let chunkModules = chunkModulesMap.get(chunkPath);
  if (!chunkModules) {
    chunkModules = new Set([moduleId]);
    chunkModulesMap.set(chunkPath, chunkModules);
  } else {
    chunkModules.add(moduleId);
  }
}

/**
 * Returns the first chunk that included a module.
 * This is used by the Node.js backend, hence why it's marked as unused in this
 * file.
 *
 * @type {GetFirstModuleChunk}
 */
function getFirstModuleChunk(moduleId) {
  const moduleChunkPaths = moduleChunksMap.get(moduleId);
  if (moduleChunkPaths == null) {
    return null;
  }

  return moduleChunkPaths.values().next().value;
}

/**
 * Removes a module from a chunk. Returns true there are no remaining chunks
 * including this module.
 *
 * @param {ModuleId} moduleId
 * @param {ChunkPath} chunkPath
 * @returns {boolean}
 */
function removeModuleFromChunk(moduleId, chunkPath) {
  const moduleChunks = moduleChunksMap.get(moduleId);
  moduleChunks.delete(chunkPath);

  const chunkModules = chunkModulesMap.get(chunkPath);
  chunkModules.delete(moduleId);

  const noRemainingModules = chunkModules.size === 0;
  if (noRemainingModules) {
    chunkModulesMap.delete(chunkPath);
  }

  const noRemainingChunks = moduleChunks.size === 0;
  if (noRemainingChunks) {
    moduleChunksMap.delete(moduleId);
  }

  return noRemainingChunks;
}

/**
 * Diposes of a chunk list and its corresponding exclusive chunks.
 *
 * @param {ChunkPath} chunkListPath
 * @returns {boolean} Whether the chunk list was disposed of.
 */
function disposeChunkList(chunkListPath) {
  const chunkPaths = chunkListChunksMap.get(chunkListPath);
  if (chunkPaths == null) {
    return false;
  }
  chunkListChunksMap.delete(chunkListPath);

  for (const chunkPath of chunkPaths) {
    const chunkChunkLists = chunkChunkListsMap.get(chunkPath);
    chunkChunkLists.delete(chunkListPath);

    if (chunkChunkLists.size === 0) {
      chunkChunkListsMap.delete(chunkPath);
      disposeChunk(chunkPath);
    }
  }

  return true;
}

/**
 * Disposes of a chunk and its corresponding exclusive modules.
 *
 * @param {ChunkPath} chunkPath
 * @returns {boolean} Whether the chunk was disposed of.
 */
function disposeChunk(chunkPath) {
  // This should happen whether or not the chunk has any modules in it. For instance,
  // CSS chunks have no modules in them, but they still need to be unloaded.
  loadedChunks.delete(chunkPath);
  BACKEND.unloadChunk(chunkPath);

  const chunkModules = chunkModulesMap.get(chunkPath);
  if (chunkModules == null) {
    return false;
  }
  chunkModules.delete(chunkPath);

  for (const moduleId of chunkModules) {
    const moduleChunks = moduleChunksMap.get(moduleId);
    moduleChunks.delete(chunkPath);

    const noRemainingChunks = moduleChunks.size === 0;
    if (noRemainingChunks) {
      moduleChunksMap.delete(moduleId);
      disposeModule(moduleId, "clear");
    }
  }

  return true;
}

/**
 * Instantiates a runtime module.
 *
 * @param {ModuleId} moduleId
 * @returns {Module}
 */
function instantiateRuntimeModule(moduleId) {
  return instantiateModule(moduleId, { type: SourceTypeRuntime });
}

/**
 * Subscribes to chunk list updates from the update server and applies them.
 *
 * @param {ChunkPath} chunkListPath
 * @param {ChunkPath[]} chunkPaths
 */
function registerChunkList(chunkListPath, chunkPaths) {
  globalThis.TURBOPACK_CHUNK_UPDATE_LISTENERS.push([
    chunkListPath,
    handleApply.bind(null, chunkListPath),
  ]);

  // Adding chunks to chunk lists and vice versa.
  const chunks = new Set(chunkPaths);
  chunkListChunksMap.set(chunkListPath, chunks);
  for (const chunkPath of chunks) {
    let chunkChunkLists = chunkChunkListsMap.get(chunkPath);
    if (!chunkChunkLists) {
      chunkChunkLists = new Set([chunkListPath]);
      chunkChunkListsMap.set(chunkPath, chunkChunkLists);
    } else {
      chunkChunkLists.add(chunkListPath);
    }
  }
}

/**
 * Registers a chunk list and marks it as a runtime chunk list. This is called
 * by the runtime of evaluated chunks.
 *
 * @param {ChunkPath} chunkListPath
 * @param {ChunkPath[]} chunkPaths
 */
function registerChunkListAndMarkAsRuntime(chunkListPath, chunkPaths) {
  registerChunkList(chunkListPath, chunkPaths);
  markChunkListAsRuntime(chunkListPath);
}

/**
 * Marks a chunk list as a runtime chunk list. There can be more than one
 * runtime chunk list. For instance, integration tests can have multiple chunk
 * groups loaded at runtime, each with its own chunk list.
 *
 * @param {ChunkPath} chunkListPath
 */
function markChunkListAsRuntime(chunkListPath) {
  runtimeChunkLists.add(chunkListPath);
}

/**
 * @param {ChunkPath} chunkPath
 */
function markChunkAsLoaded(chunkPath) {
  const chunkLoader = chunkLoaders.get(chunkPath);
  if (!chunkLoader) {
    loadedChunks.add(chunkPath);

    // This happens for all initial chunks that are loaded directly from
    // the HTML.
    return;
  }

  // Only chunks that are loaded via `loadChunk` will have a loader.
  chunkLoader.onLoad();
}

/** @type {Runtime} */
const runtime = {
  loadedChunks,
  modules: moduleFactories,
  cache: moduleCache,
  instantiateRuntimeModule,
  registerChunkList: registerChunkListAndMarkAsRuntime,
};

/**
 * @param {ChunkRegistration} chunkRegistration
 */
function registerChunk([chunkPath, chunkModules, ...run]) {
  markChunkAsLoaded(chunkPath);
  for (const [moduleId, moduleFactory] of Object.entries(chunkModules)) {
    if (!moduleFactories[moduleId]) {
      moduleFactories[moduleId] = moduleFactory;
    }
    addModuleToChunk(moduleId, chunkPath);
  }
  runnable.push(...run);
  runnable = runnable.filter((r) => r(runtime));
}

globalThis.TURBOPACK_CHUNK_UPDATE_LISTENERS =
  globalThis.TURBOPACK_CHUNK_UPDATE_LISTENERS || [];

globalThis.TURBOPACK.forEach(registerChunk);
globalThis.TURBOPACK = {
  push: registerChunk,
};
})();


//# sourceMappingURL=a587c_tests_snapshot_swc_transforms_mono_transforms_input_packages_app_index_484c8d.js.map