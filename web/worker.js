import init, * as wasm from './pkg/markit_wasm.js';
import { createMarkitCore } from './render-core.js';
export { decodeWebPatch } from './render-core.js';

let memory;
const ready = init().then(exports => { memory = exports.memory; });
let grammarModule;
const grammar = () => grammarModule ??= import('./pkg-grammars/markit_wasm.js').then(async module => {
  await module.default();
  return module;
}).catch(error => { grammarModule = undefined; throw error; });
const core = createMarkitCore(wasm, grammar);
/** Diagnostics for tests and benchmarks: live streams and Wasm linear memory. */
self.markitStats = () => ({ streams: core.streams(), lineHighlighters: core.highlighters(), wasmMemory: memory?.buffer.byteLength ?? 0 });

self.onmessage = async ({ data }) => {
  const { id } = data;
  try {
    await ready;
    const answer = await core.handle(data);
    if (answer) self.postMessage({ id, result: answer.result }, answer.transfers);
  } catch (error) {
    self.postMessage({ id, error: String(error?.message ?? error), code: error?.code });
  }
};
