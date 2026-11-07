/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::path::PathBuf;

#[derive(Debug)]
pub struct SystemInfo {
    data_directory: PathBuf,
}

impl SystemInfo {
    pub fn new(data_directory: PathBuf) -> Self {
        Self { data_directory }
    }

    pub fn refresh(&self) {}

    pub fn cpu_count(&self) -> usize {
        1
    }

    pub fn total_memory_bytes(&self) -> u64 {
        0
    }

    pub fn available_memory_bytes(&self) -> u64 {
        0
    }

    pub fn disk_total_bytes(&self) -> u64 {
        0
    }

    pub fn disk_available_bytes(&self) -> u64 {
        0
    }

    pub fn process_cpu_seconds_total(&self) -> f64 {
        0.0
    }

    pub fn process_resident_memory_bytes(&self) -> u64 {
        0
    }

    pub fn process_virtual_memory_bytes(&self) -> u64 {
        0
    }

    pub fn process_start_time_unix_seconds(&self) -> u64 {
        0
    }

    pub fn data_directory(&self) -> &PathBuf {
        &self.data_directory
    }
}
