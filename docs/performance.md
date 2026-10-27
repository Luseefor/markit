# Initial engine measurements

Measured locally on MacBook Pro Rijan on 2026-10-27 with `cargo bench -p markit --bench throughput`, Rust 1.91.1, and the release profile in this repository. Each case warms up ten times, then runs for at least 300 ms. The fixture repeats prose, headings, links, lists, tasks, tables, and fenced code.

| Operation | 261 bytes | 13,050 bytes | 104,400 bytes |
| --- | ---: | ---: | ---: |
| Native parse | 1.9 µs | 91.0 µs | 749.3 µs |
| Native parse and JSON | 2.8 µs | 126.2 µs | 971.8 µs |
| HTML | 2.2 µs | 90.5 µs | 733.8 µs |
| HTML, reused output buffer | 2.2 µs | 91.8 µs | 854.1 µs |

These are single-run engine measurements, not platform performance guarantees. They exclude JNI/C/Wasm transfers, JSON decoding in Swift/Kotlin, syntax highlighting, layout, drawing, and streaming updates. There is no comparison against Ace or another renderer yet. Buffer reuse did not improve throughput in this run; it remains an allocation-control API rather than an established speed improvement.
