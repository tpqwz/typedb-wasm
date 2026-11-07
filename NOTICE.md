# Notice

This repository is not the TypeDB project. It is an unaffiliated port of TypeDB so that the WebAssembly module Hyperbase runs can be built from published source. TypeDB has not reviewed it and does not endorse it.

## TypeDB

The database code is TypeDB Community Edition, commit `45dbd86` (VERSION 3.14.2), from <https://github.com/typedb/typedb>.

Those files keep their original copyright. They are covered by the Mozilla Public License, Version 2.0. The text is in `LICENSE`. A source file that begins with the MPL Exhibit A notice is covered software.

Modifications are in the git history of this repository. The additions written for the port are:

- `typedb-wasm/`, the WebAssembly entry
- `durability/vfs.rs`, the in-memory log
- `resource/system_info_wasm.rs`
- `compat/rocksdb/`, a small in-memory stand-in for the `rocksdb` crate

`compat/rocksdb` is not the RocksDB library. RocksDB is not distributed here.

## TypeQL

The query parser is TypeQL, commit `d07e949` (VERSION 3.14.0), from <https://github.com/typedb/typeql>. It is also MPL-2.0. It is not copied into this tree. Cargo fetches that commit. The WebAssembly module is built against it, unmodified.

The protocol checkout used while building was `a247524`, from <https://github.com/typedb/typedb-protocol>.

## Rust standard library

`typedb-wasm/patches/std-sys-time-unsupported.rs` is a modified excerpt of `library/std/src/sys/time/unsupported.rs` from the Rust project. That file is dual-licensed under MIT and Apache-2.0, with copyright held by the Rust project developers. The change is the pair of clock imports used on `wasm32-unknown-unknown`. The rest of this repository is not offered under those licenses.

## The module

`typedb-wasm/pkg/typedb_wasm_bg.wasm` is the executable form of this source. Its sha256 is `17978d8351547deda6274a0f68ed2221f153d71032f4ecf6408c349f32c62a5e`. The corresponding source is this repository.
