# Comparisons against the existing Ace libraries

These benchmarks measure Markit's current engine against the actual libraries and model code present in the adjacent Ace checkout. They do not modify Ace. Raw results are checked into `results/`; downloaded/bundled code, compiled Swift executables, and Wasm artifacts are ignored.

## Reproduce

On the benchmark Mac, install Node, Bun, Rust, matching wasm-bindgen CLI, and Playwright Chromium from `web/`. The native comparison requires Swift/Foundation and the host Rust library.

```sh
ACE_ROOT=/path/to/ace ./benchmarks/run.sh
```

Run on an idle machine. The script executes browser and native measurements sequentially. Browser measurements use headless Chromium, not Node execution of browser libraries. A local server sets COOP/COEP headers for higher-resolution performance timing. No device, simulator, or Ace process is driven.

## What each measurement includes

| Group | Markit | Existing baseline | Qualification |
| --- | --- | --- | --- |
| Markdown HTML | Rust through Wasm, input/output conversion, HTML creation | Marked 18.0.14 HTML creation | Same trusted fixture; normalized block text, headings, table rows, and task counts checked before timing |
| Lexical highlighting | Rust tokenizer, Wasm crossings, JSON serialization and JS decoding | Ace's current JS lexical highlighter | Different token metadata; Markit includes light/dark colors, Ace emits token kinds. Both preserve exact source |
| Full grammars | Optional Rust/Sublime grammar engine plus Wasm and JSON | Prism 1.30.0 through Ace's actual colored-line wrapper | Different grammars; both paths return colored source. Grammar loading is outside steady-state timing |
| Native preparation | Rust parse, C bridge, serialization, Swift JSON decoding | Actual Ace Swift block parser with Foundation inline parsing | Same common-syntax input, different presentation data. Host macOS execution, not iPhone layout |
| Foundation reference | Same Markit native path | Foundation full AttributedString Markdown | Reference parser, not Ace's exact composition; output structures differ |
| Math | Markdown parsing, vector math layout, SVG, and native PNG creation | KaTeX 0.19.0 HTML plus MathML string creation | Different outputs and amount of work; browser layout/painting excluded, so no equivalent-output speedup claim |
| Streaming | Wasm HTML patches plus JSON decoding | Ace's actual BlockStream token patches | Same append/history; different patch contents, no speedup ratio |

The Markdown fixtures deliberately exclude code/math/raw HTML so those extra Markit features do not distort the HTML comparison. Marked's default HTML output is not sanitized; these inputs are trusted. This is not a comparison of complete secure application pipelines.

Code tokenization uses unique source suffixes to force Markit's content cache to miss. Reported code sizes identify the base source; each uncached invocation adds a short comment suffix. Repeated-file cache hits are reported separately. Full-grammar timing is never presented as equivalent to the simpler lexical path. Source preservation is checked before measurement.

Synchronous browser cases use a 50 ms warmup and nine batches lasting at least 70 ms. The async Prism path loads its grammar and completes a source-preservation call before its nine measured batches. Native cases warm up for 20 invocations and drain an autorelease pool per operation for every baseline. Results report the median of per-call batch averages. `p95_batch_ms` is the 95th percentile of those nine batch averages, not a production request-latency percentile. Streaming excludes history setup and averages 100 fresh-stream appends per batch. Results retain all batch values.

Ace browser sources are bundled with Bun without minification for this driver. Cold module/first-use measurements are single observations on localhost, not statistically sampled download/startup benchmarks. Native results are host Mac measurements. There is no Android on-device benchmark: the current Ace Android transcript uses plain Text, and device controls were disabled. Do not infer Android/iPhone FPS, memory, battery, or startup from these results.

## Current findings

See [the comparison report](../docs/benchmark-comparison.md). The measurements expose remaining work in Wasm lexical highlighting and especially optional full grammars. They do not establish that Markit wins every workload.
