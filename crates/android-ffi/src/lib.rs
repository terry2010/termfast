//! TermFast Android FFI — JNI bridge for the Rust core.
//!
//! This crate is compiled as a `cdylib` and loaded by the Android app via
//! `System.loadLibrary("termfast_android_ffi")`. All business logic is delegated
//! to `termfast-core`; this crate only handles JNI serialization, Android-specific
//! storage, and the tokio runtime.

#[cfg(target_os = "android")]
use ::jni::JavaVM;
#[cfg(target_os = "android")]
use std::sync::OnceLock;

pub mod config;
pub mod credential;
pub mod cloud_sync;
pub mod cloud_sync_logic;
pub mod event;
pub mod jni;
pub mod network;
pub mod proxy_api;
pub mod pty_api;
pub mod pairing_api;
pub mod remote_terminal;
pub mod runtime;
pub mod server_api;
pub mod vpn;

#[cfg(target_os = "android")]
static GLOBAL_JVM: OnceLock<JavaVM> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn jvm() -> Option<&'static JavaVM> {
    GLOBAL_JVM.get()
}

/// Initialize the global `JavaVM` reference. Called once from `JNI_OnLoad`.
#[cfg(target_os = "android")]
pub fn set_jvm(vm: JavaVM) -> ::jni::errors::Result<()> {
    let _ = GLOBAL_JVM.set(vm);
    Ok(())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn JNI_OnLoad(vm: JavaVM, _reserved: *mut std::ffi::c_void) -> ::jni::sys::jint {
    use crate::runtime::init_android_logging;
    let _ = crate::jni::set_jvm(vm);
    init_android_logging();
    ::jni::sys::JNI_VERSION_1_6
}
