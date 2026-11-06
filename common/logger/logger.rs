/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

#![allow(unexpected_cfgs)]

use std::{fs, path::PathBuf};

use tracing::{self, Level, dispatcher::DefaultGuard, metadata::LevelFilter};
pub use tracing::{debug, error, info, trace, warn};
#[cfg(not(target_arch = "wasm32"))]
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    EnvFilter,
    fmt::SubscriberBuilder,
};
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::fmt::writer::Tee;

use crate::{log_panic::log_panic, stdio::safe_stderr};

mod log_panic;
pub mod result;
mod stdio;

pub fn initialise_logging_global(logdir: &PathBuf) {
    debug_assert!(logdir.is_absolute());
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    #[cfg(not(target_arch = "wasm32"))]
    let subscriber = {
        fs::create_dir_all(logdir.clone()).expect("Failed to create log dir");
        let file_appender = RollingFileAppender::new(Rotation::HOURLY, logdir, "typedb.log");
        SubscriberBuilder::default()
            .with_max_level(Level::TRACE)
            .with_env_filter(filter)
            .with_writer(Tee::new(safe_stderr, file_appender))
            .with_ansi(false)
            .with_thread_ids(true)
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .finish()
    };
    #[cfg(target_arch = "wasm32")]
    let subscriber = {
        let _ = logdir;
        SubscriberBuilder::default()
            .with_max_level(Level::TRACE)
            .with_env_filter(filter)
            .with_ansi(false)
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .finish()
    };

    let result = tracing::subscriber::set_global_default(subscriber).unwrap();
    let old_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        log_panic(panic_info);
        old_panic_hook(panic_info)
    }));
    result
}

pub fn initialise_logging() -> DefaultGuard {
    let subscriber = SubscriberBuilder::default().with_max_level(Level::TRACE).finish();
    tracing::subscriber::set_default(subscriber)
}
