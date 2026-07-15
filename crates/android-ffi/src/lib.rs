//! TermFast Android FFI — JNI bridge for the Rust core.
//!
//! This crate is compiled as a `cdylib` and loaded by the Android app via
//! `System.loadLibrary("termfast_android_ffi")`. All business logic is delegated
//! to `termfast-core`; this crate only handles JNI serialization, Android-specific
//! storage, and the tokio runtime.

#[cfg(target_os = "android")]
use jni::JavaVM;
#[cfg(target_os = "android")]
use std::sync::OnceLock;
use std::ffi::c_void;

pub mod config;
pub mod credential;
pub mod event;
pub mod network;
pub mod proxy_api;
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
pub fn set_jvm(vm: JavaVM) -> jni::errors::Result<()> {
    let _ = GLOBAL_JVM.set(vm);
    Ok(())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn JNI_OnLoad(_vm: JavaVM, _reserved: *mut c_void) -> jni::sys::jint {
    use crate::runtime::init_android_logging;
    init_android_logging();
    jni::sys::JNI_VERSION_1_6
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeInit(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
) {
    crate::runtime::init_android_logging();
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativePing(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
) -> jni::sys::jint {
    42
}

#[cfg(not(target_os = "android"))]
use std::ffi::c_void;
