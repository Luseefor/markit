# Markit

A Rust Markdown engine targeting native iOS and Android text rendering and WebAssembly on the web.

This is the first implementation checkpoint. The parser and bridges are implemented. Native UI adapters, syntax highlighting, and Ace integration are still in progress. This is not yet a complete renderer package for an application.

## Implemented

- CommonMark parsing through `pulldown-cmark`, plus tables, task lists, and strikethrough.
- HTML rendering with raw HTML escaped and unsafe link/image schemes removed.
- A native render plan with plain text and semantic style ranges in UTF-16 units. The plan includes headings, nested lists and quotes, code blocks with language labels, links, image alt text, and tables.
- A length-delimited C ABI for iOS, with explicit ownership, error statuses, and support for embedded NUL bytes.
- An Android JNI entry point and a WebAssembly module exporting `renderHtml` and `renderDocument`.
- Swift and Kotlin wrappers that decode the native render plan, with initial Swift Package and Android library build definitions. These do not yet include native view adapters or packaged Rust binaries.

## Workspace

```text
crates/markit       Markdown parser, native render plan, safe HTML
crates/markit-ffi   C ABI and Android JNI bridge
crates/markit-wasm  wasm-bindgen exports
platforms/ios      Swift Package and native document wrapper
platforms/android Android library and native document wrapper
```

The core uses a streaming parser without building a recursive AST. Native documents cross the language bridge once as JSON. HTML can reuse a caller-owned output buffer. These choices reduce allocation and bridge overhead, but they do not establish end-to-end UI performance. The benchmark measures the Rust engine only.

## Build and verify

```sh
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p markit --bench throughput
```

Cross-target compilation requires the corresponding Rust targets:

```sh
rustup target add wasm32-unknown-unknown aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android
cargo build -p markit-wasm --release --target wasm32-unknown-unknown
cargo build -p markit-ffi --release --target aarch64-apple-ios
cargo build -p markit-ffi --release --target aarch64-apple-ios-sim
cargo check -p markit-ffi --target aarch64-linux-android
```

Android linking will also require an NDK linker configuration. The WebAssembly binary requires `wasm-bindgen` JavaScript glue before browser use. iOS consumers must link the Rust static library; the Swift Package does not yet distribute an XCFramework.

## API

```rust
let html = markit::render_html("# Hello\n\n**Native and web.**");
let document = markit::parse("**Hello 🦀**");
let json = markit::render_document_json("```rust\nfn main() {}\n```");
```

`Document.version` is currently `1`. Every span uses a half-open `start..end` range into rendered `text`, measured in UTF-16 code units. This matches `NSRange` and Android's string indexing. Image spans carry a destination and alt text; they do not download or decode images. Native tables currently use tab-separated text with table/header spans rather than native grid layout.

See [the feature checklist](docs/ace-feature-parity.md) for remaining work.
