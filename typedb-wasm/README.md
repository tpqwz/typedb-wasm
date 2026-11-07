# typedb-wasm

WebAssembly entry to TypeDB, the one Hyperbase runs in the browser.

This crate sits in a modified TypeDB tree. The base is TypeDB `45dbd86` (VERSION 3.14.2). TypeQL is the checkout `d07e949` (VERSION 3.14.0). The protocol checkout used by the build is `a247524`. The license of TypeDB, and of this crate, is MPL-2.0. See `LICENSE` and `NOTICE.md` at the root of the tree. This is not an official TypeDB release.

`query(name, typeql)` runs one statement, or several separated by `end;`. It stops at the first error and keeps the answers from statements that already committed. `save` and `restore` move the in-memory log. The keyspace is memory (`memfs`). There is no disk database.

## Build

`wasm32-unknown-unknown` panics in `Instant::now` and `SystemTime::now`. The replacement is `patches/std-sys-time-unsupported.rs`. Copy it over `library/std/src/sys/time/unsupported.rs` in the rust-src component, then:

```
RUSTC_BOOTSTRAP=1 cargo build -p typedb-wasm --target wasm32-unknown-unknown --release -Z build-std=std,panic_abort
```

`wasm-bindgen` emits `require("env")` for the clock. Replace that with `typedb_instant_millis` (`performance.now()`) and `typedb_unix_millis` (`Date.now()`) before the module loads. `pkg/` is that bindgen output. Its `typedb_wasm_bg.wasm` is the file Hyperbase serves, sha256 `17978d8351547deda6274a0f68ed2221f153d71032f4ecf6408c349f32c62a5e`.

Host tests, one at a time, because the log is process-global:

```
cargo test -p typedb-wasm --lib -- --test-threads=1
```
