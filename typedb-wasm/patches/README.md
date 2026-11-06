# Wasm clock

`wasm32-unknown-unknown` ships a `std` that panics in `Instant::now` and `SystemTime::now`.

Copy `std-sys-time-unsupported.rs` over the matching file in the rust-src component:

`library/std/src/sys/time/unsupported.rs`

Rebuild `std` with the module:

```
RUSTC_BOOTSTRAP=1 cargo build -p typedb-wasm --target wasm32-unknown-unknown -Z build-std=std,panic_abort
```

The wasm module then imports `env.typedb_instant_millis` (`performance.now()`, milliseconds) and `env.typedb_unix_millis` (`Date.now()`, milliseconds). `wasm-bindgen` emits `require("env")` for those. Replace that with the two functions before loading the module.
