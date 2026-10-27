# Ace feature checklist

The target is a shared engine that can replace Ace's Markdown parsing and code highlighting across native clients and the web. This checkpoint has not modified or integrated with Ace.

The checklist was informed by the local Ace Markdown modules, their behavior tests, and the native iOS Markdown model. Implementations in this repository are written independently.

| Feature | Current Markit state | Remaining work |
| --- | --- | --- |
| Headings, emphasis, strong, strike, inline code | Shared parser and HTML/native plans | Native view styling |
| Nested lists, tasks, quotes, rules | Shared parser and HTML/native plans | Native layout and task presentation |
| Tables | HTML tables; tab-separated native text with spans | Native grid, alignment, horizontal scrolling |
| Fenced and indented code | Language metadata and escaped code | Syntax highlighting, code headers, copy, line numbers |
| Whole-file source highlighting | Not implemented | Rust tokenizer, language aliases, bounded cache |
| Diff fences and file metadata | Not implemented | Parse fence info, additions/deletions, file labels |
| Links and reference definitions | CommonMark references and safe URL policy | Bare URL linking and configurable app link resolution |
| File, thread, and pull-request references | Not implemented | Host callbacks and Ace integration |
| Images | Web image tags and native alt text/image spans | Host-resolved attachment bytes and native image views |
| Inline and display math | Not implemented | Dollar and backslash delimiters, rendering and source copying |
| Streaming snapshots | Incomplete Markdown parses safely | Append API, settled block reuse, late reference invalidation |
| Worker scheduling and bounded caches | Not implemented | Wasm worker, stale-result handling, cancellation and budgets |
| Large documents and code virtualization | Not implemented | Native and web block adapters with viewport-aware layout |
| Raw HTML | Escaped in HTML output; literal in native text | Review desired Ace compatibility policy |

Native JNI compilation, iOS static-library compilation, and Wasm compilation do not prove a native app or browser renders correctly. Application-level validation belongs to the next checkpoint, once view adapters exist.
