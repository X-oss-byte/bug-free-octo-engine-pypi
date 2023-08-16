/**
 * This file acts as a dummy implementor for the interface that
 * `runtime-base.ts` expects to be available in the global scope.
 *
 * This interface will be implemented by runtime backends.
 */

declare var BACKEND: RuntimeBackend;
declare var _eval: (code: EcmascriptModuleEntry) => any;
/**
 * Adds additional properties to the `TurbopackDevBaseContext` interface.
 */
declare var augmentContext: (
  context: TurbopackDevBaseContext
) => TurbopackDevContext;
declare var commonJsRequireContext: CommonJsRequireContext;
declare var loadWebAssembly: (source: SourceInfo, wasmChunkPath: ChunkPath, imports: WebAssembly.Imports) => Exports;
