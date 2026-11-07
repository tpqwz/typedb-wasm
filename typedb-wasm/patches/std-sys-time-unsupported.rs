// Modified excerpt of library/std/src/sys/time/unsupported.rs from the Rust
// project. Copyright the Rust project developers. Dual-licensed under the
// MIT license and the Apache License, Version 2.0. The change is the pair of
// clock imports below. This file is not part of the MPL-2.0 TypeDB tree.

use crate::time::Duration;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Instant(Duration);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct SystemTime(Duration);

pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::from_secs(0));

// wasm32-unknown-unknown has no clock. The TypeDB port fills these two
// imports from JavaScript (`performance.now` and `Date.now`). Other
// unsupported targets keep the panic.
#[cfg(target_arch = "wasm32")]
mod host {
    use super::Duration;

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn typedb_instant_millis() -> f64;
        fn typedb_unix_millis() -> f64;
    }

    pub fn instant() -> Duration {
        let micros = (unsafe { typedb_instant_millis() } * 1000.0) as u64;
        Duration::from_micros(micros)
    }

    pub fn system() -> Duration {
        Duration::from_millis(unsafe { typedb_unix_millis() } as u64)
    }
}

impl Instant {
    pub fn now() -> Instant {
        #[cfg(target_arch = "wasm32")]
        {
            Instant(host::instant())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            panic!("time not implemented on this platform")
        }
    }

    pub fn checked_sub_instant(&self, other: &Instant) -> Option<Duration> {
        self.0.checked_sub(other.0)
    }

    pub fn checked_add_duration(&self, other: &Duration) -> Option<Instant> {
        Some(Instant(self.0.checked_add(*other)?))
    }

    pub fn checked_sub_duration(&self, other: &Duration) -> Option<Instant> {
        Some(Instant(self.0.checked_sub(*other)?))
    }
}

impl SystemTime {
    pub const MAX: SystemTime = SystemTime(Duration::MAX);

    pub const MIN: SystemTime = SystemTime(Duration::ZERO);

    pub fn now() -> SystemTime {
        #[cfg(target_arch = "wasm32")]
        {
            SystemTime(host::system())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            panic!("time not implemented on this platform")
        }
    }

    pub fn sub_time(&self, other: &SystemTime) -> Result<Duration, Duration> {
        self.0.checked_sub(other.0).ok_or_else(|| other.0 - self.0)
    }

    pub fn checked_add_duration(&self, other: &Duration) -> Option<SystemTime> {
        Some(SystemTime(self.0.checked_add(*other)?))
    }

    pub fn checked_sub_duration(&self, other: &Duration) -> Option<SystemTime> {
        Some(SystemTime(self.0.checked_sub(*other)?))
    }
}
