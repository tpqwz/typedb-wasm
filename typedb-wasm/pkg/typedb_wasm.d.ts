/* tslint:disable */
/* eslint-disable */

/**
 * Reads the wasm clock. `unix_ms` is wall time. `elapsed_ms` is the gap between two instants.
 */
export function clock(): string;

/**
 * How many `page` entities a reopened database still has.
 */
export function count(name: string): string;

/**
 * Opens a database through the real TypeDB database crate.
 * The keyspace and the write-ahead log are both in memory.
 */
export function probe(name: string): string;

/**
 * Runs one TypeQL query, or several separated by `end;`.
 * Schema answers are `ok`. Rows are `$name=value` lines.
 * A failure starts with `error` and stops the rest of the script.
 */
export function query(name: string, typeql_query: string): string;

/**
 * Puts a previously saved log back. Opening the same name after this replays it.
 */
export function restore(bytes: Uint8Array): string;

/**
 * Defines an entity, inserts one, and counts it back.
 */
export function roundtrip(): string;

/**
 * Bytes of one database's log. Empty if that database has not been created.
 */
export function save(name: string): Uint8Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly clock: () => [number, number];
    readonly count: (a: number, b: number) => [number, number];
    readonly probe: (a: number, b: number) => [number, number];
    readonly query: (a: number, b: number, c: number, d: number) => [number, number];
    readonly restore: (a: number, b: number) => [number, number];
    readonly roundtrip: () => [number, number];
    readonly save: (a: number, b: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_export: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export2: (a: number, b: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
