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
#[cfg(target_os = "android")]
pub fn init_android_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug),
    );
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("Rust panic: {}", info);
    }));
}

#[cfg(not(target_os = "android"))]
pub fn init_android_logging() {
    // no-op on desktop
}
