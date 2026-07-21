//! Tokio runtime + Android logger setup.

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Get or create the multi-thread tokio runtime used for all async FFI calls.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("termfast-ffi")
            .build()
            .expect("Failed to create tokio runtime")
    })
}

/// Initialize Android logger and panic hook. Called from `JNI_OnLoad`.
/// M-1: Default to Warn in release to avoid leaking SSH host/user/fingerprint to logcat.
///   Kotlin side can raise to Debug via nativeSetLogLevel when BuildConfig.DEBUG is true.
#[cfg(target_os = "android")]
pub fn init_android_logging() {
    let default_level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    };
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(default_level),
    );
    std::panic::set_hook(Box::new(|info| {
        log::error!("Rust panic: {}", info);
    }));
}

/// Dynamically set the log level (called from Kotlin based on BuildConfig.DEBUG).
#[cfg(target_os = "android")]
pub fn set_log_level(level: log::LevelFilter) {
    log::set_max_level(level);
}

#[cfg(not(target_os = "android"))]
pub fn init_android_logging() {
    // no-op on desktop
}
