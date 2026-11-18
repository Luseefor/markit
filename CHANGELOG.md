# Changelog

## 0.2.1

- Export `LineHighlighter` from `@markit/web`, backed by the renderer's shared Wasm worker or in-process instance, with `setText`, `edit`, `tokens`, `setBudget`, `advance`, and `free`/`dispose`.
- Include the grammar highlighter in the web Wasm build by default so the package ships the incremental editor API.

## 0.2.0

- Add `LineHighlighter`, an incremental grammar highlighter for editors, with a line parse budget, `set_budget`, and `advance` for idle catch-up.
- Expose it from Rust, `markit-wasm` (JavaScript), the C ABI used by Swift, and Kotlin.
- Keep parser checkpoints every 32 lines and a bounded token-run cache, so a large file costs a small per-line cache instead of a full parse tree.
- Handles can move between threads and are used by one thread at a time.
