/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Memory-only replacement for the `rocksdb` crate.
//!
//! TypeDB's storage talks to RocksDB through a small set of types. This crate
//! keeps those types and stores the pairs in a `BTreeMap`, which is what a
//! browser build can actually link. The option setters are accepted and ignored.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Default)]
pub struct Options {
    create_if_missing: bool,
}

impl Options {
    pub fn create_if_missing(&mut self, yes: bool) {
        self.create_if_missing = yes;
    }
    pub fn create_missing_column_families(&mut self, _yes: bool) {}
    pub fn set_max_background_jobs(&mut self, _n: i32) {}
    pub fn set_target_file_size_base(&mut self, _n: u64) {}
    pub fn set_write_buffer_size(&mut self, _n: usize) {}
    pub fn set_max_write_buffer_size_to_maintain(&mut self, _n: i64) {}
    pub fn set_max_write_buffer_number(&mut self, _n: i32) {}
    pub fn set_write_buffer_manager(&mut self, _manager: &WriteBufferManager) {}
    pub fn set_memtable_whole_key_filtering(&mut self, _yes: bool) {}
    pub fn set_optimize_filters_for_hits(&mut self, _yes: bool) {}
    pub fn set_compression_per_level(&mut self, _levels: &[DBCompressionType]) {}
    pub fn set_prefix_extractor(&mut self, _transform: SliceTransform) {}
    pub fn set_block_based_table_factory(&mut self, _factory: &BlockBasedOptions) {}
    pub fn enable_statistics(&mut self) {}
    pub fn set_stats_dump_period_sec(&mut self, _secs: u32) {}
}

#[derive(Clone, Copy, Debug)]
pub enum DBCompressionType {
    None,
    Snappy,
    Zlib,
    Bz2,
    Lz4,
    Lz4hc,
    Zstd,
}

#[derive(Clone, Copy, Debug)]
pub enum BlockBasedIndexType {
    BinarySearch,
    HashSearch,
    TwoLevelIndexSearch,
}

#[derive(Clone, Default)]
pub struct BlockBasedOptions;

impl BlockBasedOptions {
    pub fn set_block_cache(&mut self, _cache: &Cache) {}
    pub fn set_block_restart_interval(&mut self, _interval: i32) {}
    pub fn set_index_block_restart_interval(&mut self, _interval: i32) {}
    pub fn set_format_version(&mut self, _version: i32) {}
    pub fn set_block_size(&mut self, _size: usize) {}
    pub fn set_whole_key_filtering(&mut self, _yes: bool) {}
    pub fn set_hybrid_ribbon_filter(&mut self, _bits_per_key: f64, _bloom_before_level: i32) {}
    pub fn set_partition_filters(&mut self, _yes: bool) {}
    pub fn set_index_type(&mut self, _index_type: BlockBasedIndexType) {}
    pub fn set_optimize_filters_for_memory(&mut self, _yes: bool) {}
    pub fn set_pin_top_level_index_and_filter(&mut self, _yes: bool) {}
    pub fn set_pin_l0_filter_and_index_blocks_in_cache(&mut self, _yes: bool) {}
    pub fn set_cache_index_and_filter_blocks(&mut self, _yes: bool) {}
}

#[derive(Clone)]
pub struct SliceTransform;

impl SliceTransform {
    pub fn create_fixed_prefix(_len: usize) -> Self {
        Self
    }
}

#[derive(Clone, Default)]
pub struct ReadOptions;

impl ReadOptions {
    pub fn set_total_order_seek(&mut self, _yes: bool) {}
    pub fn set_prefix_same_as_start(&mut self, _yes: bool) {}
}

#[derive(Clone, Default)]
pub struct WriteOptions {
    _disable_wal: bool,
}

impl WriteOptions {
    pub fn disable_wal(&mut self, yes: bool) {
        self._disable_wal = yes;
    }
}

#[derive(Clone)]
pub struct Cache;

impl Cache {
    pub fn new_hyper_clock_cache(_capacity: usize, _estimated_entry_charge: usize) -> Self {
        Self
    }
    pub fn new_lru_cache(_capacity: usize) -> Self {
        Self
    }
}

#[derive(Clone)]
pub struct WriteBufferManager;

impl WriteBufferManager {
    pub fn new_write_buffer_manager(_buffer_size: usize, _allow_stall: bool) -> Self {
        Self
    }
}

#[derive(Clone)]
enum BatchOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Clone, Default)]
pub struct WriteBatch {
    ops: Vec<BatchOp>,
}

impl WriteBatch {
    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.ops.push(BatchOp::Put { key: key.as_ref().to_vec(), value: value.as_ref().to_vec() });
    }

    pub fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        self.ops.push(BatchOp::Delete { key: key.as_ref().to_vec() });
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

#[derive(Clone)]
struct Store {
    map: BTreeMap<Vec<u8>, Vec<u8>>,
}

#[derive(Clone)]
pub struct DB {
    store: Arc<Mutex<Store>>,
}

impl fmt::Debug for DB {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DB")
    }
}

impl DB {
    pub fn open(_opts: &Options, path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().join("memory.kv");
        let map = if files::exists(&path) {
            decode_pairs(&files::read_file(&path).map_err(|error| Error::new(error.to_string()))?)?
        } else {
            BTreeMap::new()
        };
        Ok(Self { store: Arc::new(Mutex::new(Store { map })) })
    }

    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<(), Error> {
        self.put_opt(key, value, &WriteOptions::default())
    }

    pub fn put_opt<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &self,
        key: K,
        value: V,
        _writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        store.map.insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        Ok(())
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Error> {
        let store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        Ok(store.map.get(key.as_ref()).cloned())
    }

    pub fn get_pinned_opt<'a, K: AsRef<[u8]>>(
        &'a self,
        key: K,
        _readopts: &ReadOptions,
    ) -> Result<Option<DBPinnableSlice<'a>>, Error> {
        let bytes = self.get(key)?;
        Ok(bytes.map(|data| DBPinnableSlice { data, _db: std::marker::PhantomData }))
    }

    pub fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        store.map.remove(key.as_ref());
        Ok(())
    }

    pub fn write_opt(&self, batch: WriteBatch, _writeopts: &WriteOptions) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        for op in batch.ops {
            match op {
                BatchOp::Put { key, value } => {
                    store.map.insert(key, value);
                }
                BatchOp::Delete { key } => {
                    store.map.remove(&key);
                }
            }
        }
        Ok(())
    }

    pub fn write(&self, batch: WriteBatch) -> Result<(), Error> {
        self.write_opt(batch, &WriteOptions::default())
    }

    pub fn iterator(&self, mode: IteratorMode<'_>) -> DBIterator<'_> {
        let store = self.store.lock().ok();
        let pairs: Vec<(Box<[u8]>, Box<[u8]>)> = store
            .as_ref()
            .map(|store| store.map.iter().map(|(k, v)| (k.clone().into_boxed_slice(), v.clone().into_boxed_slice())).collect())
            .unwrap_or_default();
        let mut index = match mode {
            IteratorMode::Start => 0,
            IteratorMode::End => pairs.len(),
            IteratorMode::From(key, Direction::Forward) => pairs.iter().position(|(k, _)| k.as_ref() >= key).unwrap_or(pairs.len()),
            IteratorMode::From(key, Direction::Reverse) => {
                pairs.iter().rposition(|(k, _)| k.as_ref() <= key).map(|i| i + 1).unwrap_or(0)
            }
        };
        if matches!(mode, IteratorMode::End) {
            index = pairs.len();
        }
        DBIterator::new(pairs, index, !matches!(mode, IteratorMode::End | IteratorMode::From(_, Direction::Reverse)))
    }

    pub fn raw_iterator(&self) -> DBRawIterator<'_> {
        self.raw_iterator_opt(ReadOptions::default())
    }

    pub fn raw_iterator_opt(&self, _readopts: ReadOptions) -> DBRawIterator<'_> {
        DBRawIterator { db: self.clone(), current: None, finished: false, error: None, _marker: std::marker::PhantomData }
    }

    pub fn property_int_value(&self, name: &str) -> Result<Option<u64>, Error> {
        let store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        let value = if name.contains("num-keys") {
            store.map.len() as u64
        } else {
            store.map.iter().map(|(k, v)| (k.len() + v.len()) as u64).sum()
        };
        Ok(Some(value))
    }

    fn pairs(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
        let store = self.store.lock().map_err(|_| Error::new("store lock poisoned"))?;
        Ok(store.map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

pub struct DBPinnableSlice<'a> {
    data: Vec<u8>,
    _db: std::marker::PhantomData<&'a ()>,
}

impl AsRef<[u8]> for DBPinnableSlice<'_> {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Deref for DBPinnableSlice<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.data
    }
}

pub struct DBIterator<'a> {
    pairs: Vec<(Box<[u8]>, Box<[u8]>)>,
    index: usize,
    forward: bool,
    _db: std::marker::PhantomData<&'a DB>,
}

impl<'a> DBIterator<'a> {
    fn new(pairs: Vec<(Box<[u8]>, Box<[u8]>)>, index: usize, forward: bool) -> Self {
        Self { pairs, index, forward, _db: std::marker::PhantomData }
    }
}

impl Iterator for DBIterator<'_> {
    type Item = Result<(Box<[u8]>, Box<[u8]>), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.forward {
            if self.index >= self.pairs.len() {
                return None;
            }
            let item = self.pairs[self.index].clone();
            self.index += 1;
            Some(Ok(item))
        } else if self.index == 0 {
            None
        } else {
            self.index -= 1;
            Some(Ok(self.pairs[self.index].clone()))
        }
    }
}

pub struct DBRawIterator<'a> {
    db: DB,
    current: Option<(Vec<u8>, Vec<u8>)>,
    finished: bool,
    error: Option<Error>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> DBRawIterator<'a> {
    pub fn valid(&self) -> bool {
        self.current.is_some()
    }

    pub fn status(&self) -> Result<(), Error> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub fn seek<K: AsRef<[u8]>>(&mut self, key: K) {
        self.land(|pairs| pairs.iter().find(|(k, _)| k.as_slice() >= key.as_ref()).cloned());
    }

    pub fn seek_for_prev<K: AsRef<[u8]>>(&mut self, key: K) {
        self.land(|pairs| pairs.iter().rev().find(|(k, _)| k.as_slice() <= key.as_ref()).cloned());
    }

    pub fn seek_to_first(&mut self) {
        self.land(|pairs| pairs.first().cloned());
    }

    pub fn seek_to_last(&mut self) {
        self.land(|pairs| pairs.last().cloned());
    }

    pub fn next(&mut self) {
        let after = self.current.as_ref().map(|(k, _)| k.clone());
        self.land(|pairs| match after {
            Some(key) => pairs.iter().find(|(k, _)| k.as_slice() > key.as_slice()).cloned(),
            None => pairs.first().cloned(),
        });
    }

    pub fn prev(&mut self) {
        let before = self.current.as_ref().map(|(k, _)| k.clone());
        self.land(|pairs| match before {
            Some(key) => pairs.iter().rev().find(|(k, _)| k.as_slice() < key.as_slice()).cloned(),
            None => pairs.last().cloned(),
        });
    }

    pub fn key(&self) -> Option<&[u8]> {
        self.current.as_ref().map(|(k, _)| k.as_slice())
    }

    pub fn value(&self) -> Option<&[u8]> {
        self.current.as_ref().map(|(_, v)| v.as_slice())
    }

    pub fn item(&self) -> Option<(&[u8], &[u8])> {
        self.current.as_ref().map(|(k, v)| (k.as_slice(), v.as_slice()))
    }

    fn land(&mut self, pick: impl FnOnce(&[(Vec<u8>, Vec<u8>)]) -> Option<(Vec<u8>, Vec<u8>)>) {
        match self.db.pairs() {
            Ok(pairs) => {
                self.current = pick(&pairs);
                self.finished = self.current.is_none();
                self.error = None;
            }
            Err(error) => {
                self.current = None;
                self.finished = true;
                self.error = Some(error);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum Direction {
    Forward,
    Reverse,
}

pub enum IteratorMode<'a> {
    Start,
    End,
    From(&'a [u8], Direction),
}

pub mod checkpoint {
    use super::{DB, Error, encode_pairs, files};
    use std::path::Path;

    pub struct Checkpoint<'a> {
        db: &'a DB,
    }

    impl<'a> Checkpoint<'a> {
        pub fn new(db: &'a DB) -> Result<Self, Error> {
            Ok(Self { db })
        }

        pub fn create_checkpoint<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
            let path = path.as_ref();
            files::create_dir_all(path).map_err(|error| Error::new(error.to_string()))?;
            let bytes = encode_pairs(&self.db.pairs()?);
            files::write_file(&path.join("memory.kv"), &bytes).map_err(|error| Error::new(error.to_string()))
        }
    }
}

// Silence the unused-field warning on the iterator constructor helper until a caller needs it.
#[allow(dead_code)]
fn _keep(iterator: DBIterator<'_>) {
    let _ = DBIterator::new(iterator.pairs, iterator.index, iterator.forward);
}

fn encode_pairs(pairs: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (key, value) in pairs {
        bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value);
    }
    bytes
}

fn decode_pairs(mut bytes: &[u8]) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, Error> {
    let mut map = BTreeMap::new();
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err(Error::new("checkpoint truncated"));
        }
        let key_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        bytes = &bytes[4..];
        if bytes.len() < key_len + 4 {
            return Err(Error::new("checkpoint truncated"));
        }
        let key = bytes[..key_len].to_vec();
        bytes = &bytes[key_len..];
        let value_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        bytes = &bytes[4..];
        if bytes.len() < value_len {
            return Err(Error::new("checkpoint truncated"));
        }
        let value = bytes[..value_len].to_vec();
        bytes = &bytes[value_len..];
        map.insert(key, value);
    }
    Ok(map)
}

#[cfg(any(target_arch = "wasm32", feature = "memfs"))]
mod files {
    pub use durability::vfs::{create_dir_all, exists, read_file, write_file};
}

#[cfg(not(any(target_arch = "wasm32", feature = "memfs")))]
mod files {
    use std::fs;
    use std::io;
    use std::path::Path;

    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::write(path, bytes)
    }
}
