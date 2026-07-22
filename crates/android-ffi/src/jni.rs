//! JNI function implementations for TermFast Android FFI.
//!
//! All `Java_com_termfast_app_RustBridge_*` functions are declared here.
//! They bridge Kotlin calls to `termfast-core` business logic.

#![cfg(target_os = "android")]

use crate::runtime::runtime;
use ::jni::objects::{JClass, JObject, JString, GlobalRef};
use ::jni::sys::{jboolean, jint, jstring, JNI_FALSE, JNI_TRUE};
use ::jni::JNIEnv;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::Mutex;
use termfast_core::config::{Config, ConfigManager, ServerConfig as CoreServerConfig, TriggerInstance};
use termfast_core::server::ServerInstance;
use termfast_credential::CredentialStore;

/// Global state holder for the FFI layer.
pub struct FfiState {
    pub data_dir: String,
    pub config_manager: Option<ConfigManager>,
    pub servers: std::collections::HashMap<String, Arc<ServerInstance>>,
    pub event_callback: Option<GlobalRef>,
    /// Ports reserved by running proxies (port → server_id).
    /// Updated atomically with the conflict check under the state lock.
    pub reserved_ports: std::collections::HashMap<u16, String>,
}

static STATE: OnceLock<Mutex<FfiState>> = OnceLock::new();

/// Last connection error detail (set by nativeConnectServer on failure).
static LAST_CONNECT_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

pub fn last_connect_error() -> &'static Mutex<String> {
    LAST_CONNECT_ERROR.get_or_init(|| Mutex::new(String::new()))
}

/// Last connection error code (e.g. "AuthFailed", "HostKeyMismatch").
static LAST_ERROR_CODE: OnceLock<Mutex<String>> = OnceLock::new();

pub fn last_error_code() -> &'static Mutex<String> {
    LAST_ERROR_CODE.get_or_init(|| Mutex::new(String::new()))
}

/// Last connection error raw detail (before formatting).
static LAST_ERROR_RAW: OnceLock<Mutex<String>> = OnceLock::new();

pub fn last_error_raw() -> &'static Mutex<String> {
    LAST_ERROR_RAW.get_or_init(|| Mutex::new(String::new()))
}

/// Format an error code + detail into a user-friendly Chinese message.
/// Mirrors the Kotlin ErrorMessages.format() logic.
fn format_error_message(code: &str, detail: &str) -> String {
    let d = detail.to_lowercase();
    match code {
        "SshConnectFailed" => {
            if d.contains("timed out") || d.contains("timeout") {
                "连接超时，请检查服务器地址和端口是否正确，以及网络是否畅通".to_string()
            } else if d.contains("connection refused") {
                "服务器拒绝连接，可能是 SSH 服务未启动或端口号错误".to_string()
            } else if d.contains("unreachable") || d.contains("noroutetohost") {
                "网络不可达，请检查本地网络或 VPN 是否正常".to_string()
            } else if d.contains("dns") || d.contains("name or service not known") {
                "域名解析失败，请检查服务器地址是否正确".to_string()
            } else if d.contains("reset") || d.contains("broken pipe") {
                "连接被重置，可能是网络不稳定或服务器主动断开".to_string()
            } else {
                format!("无法连接到服务器：{}", detail)
            }
        }
        "AuthFailed" => {
            if d.contains("key file not found") {
                "密钥文件不存在，请检查密钥路径".to_string()
            } else if d.contains("failed to load key") {
                "密钥加载失败，可能是文件格式错误或密码短语不正确".to_string()
            } else {
                "用户名或密码错误，请重新输入".to_string()
            }
        }
        "CredentialNotFound" => {
            if d.contains("key file") {
                "密钥文件不存在，请检查密钥路径".to_string()
            } else {
                "未找到保存的凭据，请重新输入密码".to_string()
            }
        }
        "HostKeyMismatch" => {
            "服务器主机密钥已变更，可能服务器重装了系统或存在中间人攻击，请确认安全后重新连接".to_string()
        }
        _ => format!("连接失败：{}", detail),
    }
}

pub fn state() -> &'static Mutex<FfiState> {
    STATE.get_or_init(|| {
        Mutex::new(FfiState {
            data_dir: String::new(),
            config_manager: None,
            servers: std::collections::HashMap::new(),
            event_callback: None,
            reserved_ports: std::collections::HashMap::new(),
        })
    })
}

fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s)
        .map(|cs| cs.to_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// Like jstring_to_string but returns Zeroizing<String> for sensitive data (L-10)
fn jstring_to_secret(env: &mut JNIEnv, s: &JString) -> zeroize::Zeroizing<String> {
    zeroize::Zeroizing::new(jstring_to_string(env, s))
}

fn string_to_jstring<'a>(env: &mut JNIEnv<'a>, s: &str) -> JString<'a> {
    env.new_string(s).unwrap_or_else(|_| env.new_string("").unwrap())
}

fn bool_to_jbool(b: bool) -> jboolean {
    if b { JNI_TRUE } else { JNI_FALSE }
}

/// Dispatch an event JSON to the Kotlin callback (if set).
/// Called from the tracing layer and other event emitters.
pub fn dispatch_event_to_kotlin(json: &str) {
    let callback = {
        let st = state().lock().unwrap();
        st.event_callback.clone()
    };
    if let Some(global) = callback {
        let vm = match JNI_JVM.get() {
            Some(vm) => vm,
            None => return,
        };
        let mut env = match vm.attach_current_thread() {
            Ok(e) => e,
            Err(_) => return,
        };
        let listener = global.as_obj();
        let json_jstring = env.new_string(json).unwrap_or_else(|_| env.new_string("").unwrap());
        let _ = env.call_method(listener, "onEvent", "(Ljava/lang/String;)V", &[::jni::objects::JValue::Object(&json_jstring)]);
    }
}

/// Convenience: send a log entry to Kotlin (if callback is set).
#[cfg(target_os = "android")]
pub fn log_to_kotlin(level: &str, msg: &str) {
    crate::event::RustEvent::log(level, "termfast", msg);
}

// === JNI_OnLoad JVM storage ===

#[cfg(target_os = "android")]
static JNI_JVM: OnceLock<::jni::JavaVM> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_jvm(vm: ::jni::JavaVM) {
    let _ = JNI_JVM.set(vm);
}

// === Native functions ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeInit(
    mut _env: JNIEnv,
    _class: JClass,
) {
    crate::runtime::init_android_logging();
}

/// Set the Rust log level (M-1: Kotlin calls this with Debug or Warn based on BuildConfig.DEBUG)
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSetLogLevel(
    mut _env: JNIEnv,
    _class: JClass,
    level: jint,
) {
    let filter = match level {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Warn,
    };
    crate::runtime::set_log_level(filter);
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativePing(
    mut _env: JNIEnv,
    _class: JClass,
) -> jint {
    42
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSetDataDir(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    let dir = jstring_to_string(&mut env, &path);
    // Initialize credential store for this data directory
    crate::credential::init_credential_store(&dir);
    // Initialize config manager for this data directory
    let path_buf = std::path::PathBuf::from(&dir);
    // Do all async work OUTSIDE the state lock to avoid blocking
    // other JNI calls during startup.
    let (servers, config_manager) = if let Ok(cm) = crate::config::config_manager_for_dir(path_buf) {
        let rt = runtime();
        let config = cm.get_blocking();
        let templates = config.trigger_templates.clone();
        let mut servers = std::collections::HashMap::new();
        for server in config.servers.iter() {
            let instance = Arc::new(ServerInstance::new(server.clone()));
            let _ = rt.block_on(instance.set_trigger_templates(templates.clone()));
            let _ = rt.block_on(instance.set_triggers(server.triggers.clone()));
            let _ = rt.block_on(instance.set_socket_protector(Arc::new(crate::network::AndroidSocketProtector)));
            servers.insert(server.id.clone(), instance);
        }
        (servers, Some(cm))
    } else {
        (std::collections::HashMap::new(), None)
    };
    // Now briefly acquire the lock to update state.
    let mut st = state().lock().unwrap();
    st.data_dir = dir;
    st.servers = servers;
    st.config_manager = config_manager;
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGetConfigJson(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let st = state().lock().unwrap();
    let json = if let Some(ref cm) = st.config_manager {
        let config = cm.get_blocking();
        serde_json::to_string(&config).unwrap_or_default()
    } else {
        serde_json::to_string(&Config::default()).unwrap_or_default()
    };
    string_to_jstring(&mut env, &json).into_raw()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSaveConfigJson(
    mut env: JNIEnv,
    _class: JClass,
    json: JString,
) -> jboolean {
    let json_str = jstring_to_string(&mut env, &json);
    let result = serde_json::from_str::<Config>(&json_str)
        .map(|cfg| {
            let st = state().lock().unwrap();
            if let Some(ref cm) = st.config_manager {
                let rt = runtime();
                rt.block_on(cm.modify(|c| *c = cfg))
                    .map_err(|e| std::io::Error::other(e.to_string()))
            } else {
                Ok(())
            }
        });
    bool_to_jbool(result.is_ok())
}

// === Server lifecycle ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeAddServer(
    mut env: JNIEnv,
    _class: JClass,
    json: JString,
) -> jstring {
    let json_str = jstring_to_string(&mut env, &json);
    let server: CoreServerConfig = match serde_json::from_str(&json_str) {
        Ok(s) => s,
        Err(_) => return string_to_jstring(&mut env, "").into_raw(),
    };
    let id = server.id.clone();
    let mut st = state().lock().unwrap();
    let templates = if let Some(ref cm) = st.config_manager {
        let rt = runtime();
        let _ = rt.block_on(cm.modify(|cfg| {
            cfg.servers.push(server.clone());
        }));
        let cfg = cm.get_blocking();
        cfg.trigger_templates.clone()
    } else {
        Vec::new()
    };
    // Create a ServerInstance for runtime management
    let instance = Arc::new(ServerInstance::new(server));
    let rt = runtime();
    let _ = rt.block_on(instance.set_trigger_templates(templates));
    let _ = rt.block_on(instance.set_socket_protector(Arc::new(crate::network::AndroidSocketProtector)));
    st.servers.insert(id.clone(), instance);
    string_to_jstring(&mut env, &id).into_raw()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeUpdateServer(
    mut env: JNIEnv,
    _class: JClass,
    json: JString,
) -> jboolean {
    let json_str = jstring_to_string(&mut env, &json);
    let server: CoreServerConfig = match serde_json::from_str(&json_str) {
        Ok(s) => s,
        Err(_) => return bool_to_jbool(false),
    };
    let id = server.id.clone();
    let mut st = state().lock().unwrap();
    // Update config
    let templates = if let Some(ref cm) = st.config_manager {
        let rt = runtime();
        let _ = rt.block_on(cm.modify(|cfg| {
            if let Some(slot) = cfg.servers.iter_mut().find(|s| s.id == id) {
                *slot = server.clone();
            }
        }));
        cm.get_blocking().trigger_templates.clone()
    } else {
        Vec::new()
    };
    // Rebuild the ServerInstance so runtime uses the updated config (host, port, etc.)
    let instance = Arc::new(ServerInstance::new(server));
    let rt = runtime();
    let _ = rt.block_on(instance.set_trigger_templates(templates));
    let _ = rt.block_on(instance.set_socket_protector(Arc::new(crate::network::AndroidSocketProtector)));
    st.servers.insert(id, instance);
    bool_to_jbool(true)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeRemoveServer(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    // Remove and disconnect the runtime instance first, so SSH/proxy/VPN
    // are properly torn down before we discard the Arc reference.
    // Without this, the old SSH connection stays alive as an orphan and
    // the proxy keeps listening on its port, causing the edited server
    // to appear "still connected" even with wrong credentials.
    let instance = {
        let mut st = state().lock().unwrap();
        st.servers.remove(&id_str)
    };
    if let Some(instance) = instance {
        let rt = runtime();
        let _ = rt.block_on(async {
            // Stop tun2proxy (VPN tunnel) — this is a global operation,
            // not per-instance.
            let _ = crate::vpn::stop_tun2proxy().await;
            // Stop the SOCKS5/HTTP proxy on this instance
            let _ = instance.stop_proxy().await;
            // Disconnect SSH
            let _ = instance.disconnect().await;
        });
        // Release reserved ports
        release_ports(&id_str);
    }
    // Remove from config
    let st = state().lock().unwrap();
    if let Some(ref cm) = st.config_manager {
        let rt = runtime();
        let _ = rt.block_on(cm.modify(|cfg| {
            cfg.servers.retain(|s| s.id != id_str);
        }));
    }
    let data_dir = st.data_dir.clone();
    drop(st);
    // Clean up credentials and key files (H-1/L-3: was not cleaned up)
    let store = crate::credential::android_credential_store();
    let _ = store.delete(&termfast_credential::make_key(&id_str, "password"));
    let _ = store.delete(&termfast_credential::make_key(&id_str, "key"));
    let _ = store.delete(&termfast_credential::make_key(&id_str, "key_passphrase"));
    // Remove the key directory (private key file on disk)
    if !data_dir.is_empty() {
        let key_dir = std::path::PathBuf::from(&data_dir).join("keys").join(&id_str);
        let _ = std::fs::remove_dir_all(&key_dir);
    }
    bool_to_jbool(true)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeListServers(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let st = state().lock().unwrap();
    let json = if let Some(ref cm) = st.config_manager {
        let config = cm.get_blocking();
        serde_json::to_string(&config.servers).unwrap_or_default()
    } else {
        serde_json::to_string::<Vec<CoreServerConfig>>(&vec![]).unwrap_or_default()
    };
    string_to_jstring(&mut env, &json).into_raw()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeConnectServer(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jboolean {
    use termfast_core::ssh::auth::AuthMethod;

    let id_str = jstring_to_string(&mut _env, &id);
    // Validate server_id to prevent path traversal (L-7: consistent with generate_keypair_at)
    if !id_str.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        tracing::error!("nativeConnectServer: invalid server_id '{}'", id_str);
        *last_error_code().lock().unwrap() = "InvalidParams".to_string();
        *last_error_raw().lock().unwrap() = "无效的服务器 ID".to_string();
        *last_connect_error().lock().unwrap() = "无效的服务器 ID".to_string();
        return bool_to_jbool(false);
    }
    let (instance, data_dir) = {
        let st = state().lock().unwrap();
        let instance = match st.servers.get(&id_str).cloned() {
            Some(i) => i,
            None => {
                tracing::error!("nativeConnectServer: server instance not found for id={}", id_str);
                return bool_to_jbool(false);
            }
        };
        (instance, st.data_dir.clone())
    };
    tracing::info!("nativeConnectServer: host={} port={} user={} auth_method={}",
        instance.config.ssh.host, instance.config.ssh.port,
        instance.config.ssh.user, instance.config.ssh.auth_method);

    let store = crate::credential::android_credential_store();
    let auth = if instance.config.ssh.auth_method == "key" {
        let key_content = store.load(&termfast_credential::make_key(&id_str, "key")).unwrap_or_default();
        let passphrase = store.load(&termfast_credential::make_key(&id_str, "key_passphrase"))
            .ok()
            .map(zeroize::Zeroizing::new);
        if key_content.is_empty() {
            tracing::error!("No key credential for {}", id_str);
            *last_error_code().lock().unwrap() = "MissingCredential".to_string();
            *last_error_raw().lock().unwrap() = "未保存密钥，请先在服务器设置中输入密钥".to_string();
            *last_connect_error().lock().unwrap() = "未保存密钥，请先在服务器设置中输入密钥".to_string();
            return bool_to_jbool(false);
        }
        let key_dir = std::path::PathBuf::from(&data_dir).join("keys").join(&id_str);
        let _ = std::fs::create_dir_all(&key_dir);
        let key_path = key_dir.join("id_ed25519");
        let _ = std::fs::write(&key_path, &key_content);
        // Set 0600 permissions on the private key file (H-1: was missing)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
        }
        AuthMethod::Key {
            key_path: key_path.to_string_lossy().to_string(),
            passphrase,
        }
    } else {
        let password = store.load(&termfast_credential::make_key(&id_str, "password")).unwrap_or_default();
        if password.is_empty() {
            tracing::error!("No password credential for {}", id_str);
            *last_error_code().lock().unwrap() = "MissingCredential".to_string();
            *last_error_raw().lock().unwrap() = "未保存密码，请先在服务器设置中输入密码".to_string();
            *last_connect_error().lock().unwrap() = "未保存密码，请先在服务器设置中输入密码".to_string();
            return bool_to_jbool(false);
        }
        tracing::info!("nativeConnectServer: password loaded");
        AuthMethod::Password { password: zeroize::Zeroizing::new(password) }
    };

    let rt = runtime();
    log_to_kotlin("info", &format!("Connecting to SSH server {}...", id_str));
    let result = rt.block_on(async {
        match instance.connect(&auth).await {
            Ok(()) => {
                instance.start_connection_monitor().await;
                // Persist host key fingerprint if this was a first connection (TOFU).
                if let Some(fp) = instance.get_host_key_fingerprint().await {
                    tracing::info!("nativeConnectServer: persisting host key fingerprint: {}", fp);
                    let mgr = {
                        let st = state().lock().unwrap();
                        st.config_manager.clone()
                    };
                    if let Some(mgr) = mgr {
                        let _ = mgr.modify(|config| {
                            if let Some(srv) = config.servers.iter_mut().find(|s| s.id == id_str) {
                                if srv.ssh.host_key_fingerprint.is_none() {
                                    srv.ssh.host_key_fingerprint = Some(fp);
                                }
                            }
                        }).await;
                    }
                }
                log_to_kotlin("info", "SSH connected successfully");
                Ok(())
            }
            Err(e) => {
                let (status, code, detail) = match &e {
                    termfast_core::error::Error::Ipc(ipc) => {
                        let st = if ipc.code == termfast_core::error::ErrorCode::AuthFailed {
                            "auth_failed"
                        } else {
                            "offline"
                        };
                        (st, format!("{:?}", ipc.code), ipc.detail.clone())
                    }
                    _ => ("offline", "Internal".to_string(), format!("{:?}", e)),
                };
                log_to_kotlin("error", &format!("SSH connect failed: {}", detail));
                tracing::error!("nativeConnectServer: SSH connect failed: code={} detail={}", code, detail);
                // Store the user-friendly error message so Kotlin can
                // retrieve it via nativeGetLastError() after a failed
                // nativeConnectServer call.
                let user_msg = format_error_message(&code, &detail);
                *last_connect_error().lock().unwrap() = user_msg;
                *last_error_code().lock().unwrap() = code.clone();
                *last_error_raw().lock().unwrap() = detail.clone();
                // Emit status_changed event with error details so the UI
                // can show a user-friendly localized message.
                let event = crate::event::RustEvent::ServerStatusChanged {
                    server_id: id_str.clone(),
                    status: status.to_string(),
                    exit_ip: None,
                    latency_ms: None,
                    error_code: Some(code),
                    error_detail: Some(detail),
                };
                crate::jni::dispatch_event_to_kotlin(&event.to_json());
                Err(e)
            }
        }
    });
    bool_to_jbool(result.is_ok())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeDisconnectServer(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(&id_str).cloned()
    };
    if let Some(instance) = instance {
        let rt = runtime();
        let _ = rt.block_on(instance.disconnect());
        // Clean up the private key file from disk after disconnect (H-1)
        let data_dir = {
            let st = state().lock().unwrap();
            st.data_dir.clone()
        };
        if !data_dir.is_empty() {
            let key_dir = std::path::PathBuf::from(&data_dir).join("keys").join(&id_str);
            let _ = std::fs::remove_dir_all(&key_dir);
        }
        bool_to_jbool(true)
    } else {
        bool_to_jbool(false)
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGetServerStatus(
    mut env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jstring {
    let id_str = jstring_to_string(&mut env, &id);
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(&id_str).cloned()
    };
    let status = if let Some(instance) = instance {
        let rt = runtime();
        let s = rt.block_on(instance.status());
        serde_json::json!({
            "server_id": id_str,
            "status": format!("{:?}", s).to_lowercase(),
        })
        .to_string()
    } else {
        serde_json::json!({
            "server_id": id_str,
            "status": "disconnected",
        })
        .to_string()
    };
    string_to_jstring(&mut env, &status).into_raw()
}

// === Proxy ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeStartProxy(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
    _socks5_port: jint,
    _http_port: jint,
    _mixed_port: jint,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    // Atomically check for port conflicts and reserve ports under the state lock.
    // This prevents two concurrent start_proxy calls from both succeeding.
    // NOTE: log_to_kotlin dispatches events via state().lock() too, so we must
    // not call it while holding the lock — collect the error first, release
    // the lock, then log.
    let (instance, port_conflict_msg) = {
        let mut st = state().lock().unwrap();
        let instance = st.servers.get(&id_str).cloned();
        let mut conflict: Option<String> = None;
        if let Some(ref inst) = instance {
            // Collect ports this server will use
            let socks5 = inst.config.proxy.socks5_port;
            let http = inst.config.proxy.http_port;
            let mixed = inst.config.proxy.mixed_port;
            let mut ports_to_reserve = Vec::new();
            if mixed > 0 {
                ports_to_reserve.push(mixed);
            } else {
                ports_to_reserve.push(socks5);
                if http > 0 { ports_to_reserve.push(http); }
            }
            // Check if any port is already reserved by another server
            for &port in &ports_to_reserve {
                if let Some(owner) = st.reserved_ports.get(&port) {
                    if owner != &id_str {
                        conflict = Some(format!("端口 {} 被其他服务器占用，请修改端口配置", port));
                        break;
                    }
                }
            }
            if conflict.is_none() {
                // Reserve the ports
                for port in ports_to_reserve {
                    st.reserved_ports.insert(port, id_str.clone());
                }
            }
        }
        (instance, conflict)
    };
    // Log outside the state lock to avoid deadlock with dispatch_event_to_kotlin
    if let Some(msg) = port_conflict_msg {
        tracing::error!("{}", msg);
        log_to_kotlin("error", &msg);
        *last_error_code().lock().unwrap() = "PortInUse".to_string();
        *last_error_raw().lock().unwrap() = msg;
        return bool_to_jbool(false);
    }
    if let Some(instance) = instance {
        let rt = runtime();
        log_to_kotlin("info", &format!("Starting proxy for server {}...", id_str));
        match rt.block_on(instance.start_proxy()) {
            Ok(_) => {
                log_to_kotlin("info", "Proxy started successfully");
                bool_to_jbool(true)
            }
            Err(e) => {
                // Release reserved ports on failure
                release_ports(&id_str);
                log_to_kotlin("error", &format!("Proxy start failed: {:?}", e));
                bool_to_jbool(false)
            }
        }
    } else {
        bool_to_jbool(false)
    }
}

/// Release all ports reserved by a server.
fn release_ports(server_id: &str) {
    let mut st = state().lock().unwrap();
    st.reserved_ports.retain(|_, owner| owner != server_id);
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeStopProxy(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(&id_str).cloned()
    };
    if let Some(instance) = instance {
        let rt = runtime();
        let _ = rt.block_on(instance.stop_proxy());
        // Release reserved ports
        release_ports(&id_str);
        bool_to_jbool(true)
    } else {
        bool_to_jbool(false)
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeIsProxyRunning(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(&id_str).cloned()
    };
    if let Some(instance) = instance {
        let rt = runtime();
        let running = rt.block_on(instance.is_proxy_running());
        bool_to_jbool(running)
    } else {
        bool_to_jbool(false)
    }
}

// === VPN ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeStartVpn(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
    _tun_fd: jint,
    _mtu: jint,
    _socks5_port: jint,
    dns_strategy: JString,
    ipv6_enabled: jboolean,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    let tun_fd = _tun_fd;
    let mtu = _mtu as u16;
    let socks5_port = _socks5_port as u16;
    let dns = jstring_to_string(&mut _env, &dns_strategy);
    let ipv6 = ipv6_enabled == JNI_TRUE;
    log_to_kotlin("info", &format!("Starting tun2proxy: fd={}, mtu={}, socks5={}, dns={}, ipv6={}",
        tun_fd, mtu, socks5_port, dns, ipv6));
    let rt = runtime();
    // Spawn the tun2proxy task — it runs until stop_tun2proxy is called
    let handle = rt.spawn(async move {
        if let Err(e) = crate::vpn::start_tun2proxy(tun_fd, mtu, socks5_port, &dns, ipv6).await {
            log_to_kotlin("error", &format!("tun2proxy failed: {:?}", e));
        }
    });
    // Store the task handle so stop_tun2proxy can await it
    crate::vpn::set_vpn_task(handle);
    bool_to_jbool(true)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeStopVpn(
    mut _env: JNIEnv,
    _class: JClass,
    _id: JString,
) -> jboolean {
    tracing::info!("nativeStopVpn");
    let rt = runtime();
    rt.block_on(crate::vpn::stop_tun2proxy());
    bool_to_jbool(true)
}

/// Returns the last connection error message (user-friendly Chinese),
/// or empty string if no error. Consumed by Kotlin after a failed
/// nativeConnectServer call.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGetLastError<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
) -> JString<'a> {
    let msg = last_connect_error().lock().unwrap().clone();
    env.new_string(&msg).unwrap_or_else(|_| env.new_string("").unwrap())
}

/// Returns the last connection error code (e.g. "AuthFailed", "HostKeyMismatch"),
/// or empty string if no error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGetLastErrorCode<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
) -> JString<'a> {
    let code = last_error_code().lock().unwrap().clone();
    env.new_string(&code).unwrap_or_else(|_| env.new_string("").unwrap())
}

/// Returns the last connection error raw detail (before formatting),
/// or empty string if no error. Used to extract fingerprints for HostKeyMismatch.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGetLastErrorRaw<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
) -> JString<'a> {
    let raw = last_error_raw().lock().unwrap().clone();
    env.new_string(&raw).unwrap_or_else(|_| env.new_string("").unwrap())
}

/// Accept a new host key after user confirmation (server was reinstalled, etc.)
/// Updates the SSH client's known key and persists the fingerprint to config.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeAcceptHostKey(
    mut _env: JNIEnv,
    _class: JClass,
    id: JString,
    fingerprint: JString,
) -> jboolean {
    let id_str = jstring_to_string(&mut _env, &id);
    let fp_str = jstring_to_string(&mut _env, &fingerprint);
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(&id_str).cloned()
    };
    if let Some(instance) = instance {
        let rt = runtime();
        rt.block_on(async {
            instance.accept_host_key(fp_str.clone()).await;
            // Persist to config
            let mgr = {
                let st = state().lock().unwrap();
                st.config_manager.clone()
            };
            if let Some(mgr) = mgr {
                let _ = mgr.modify(|config| {
                    if let Some(srv) = config.servers.iter_mut().find(|s| s.id == id_str) {
                        srv.ssh.host_key_fingerprint = Some(fp_str);
                    }
                }).await;
            }
        });
        bool_to_jbool(true)
    } else {
        bool_to_jbool(false)
    }
}

// === Event subscription ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSetEventListener(
    env: JNIEnv,
    _class: JClass,
    listener: JObject,
) {
    // Create a global reference to the listener object so it survives across JNI calls.
    // The previous GlobalRef (if any) is dropped automatically, which calls
    //   DeleteGlobalRef via jni crate's Drop impl.
    let global = env.new_global_ref(listener).ok();
    let mut st = state().lock().unwrap();
    st.event_callback = global;

    // Also set the core platform event callback so core can emit events
    termfast_core::platform::set_event_callback(std::sync::Arc::new(|json: &str| {
        dispatch_event_to_kotlin(json);
    }));
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSetProtectCallback(
    env: JNIEnv,
    _class: JClass,
    vpn_service: JObject,
) {
    // Store a global ref to the VpnService so we can call protect(fd) on it
    let global = match env.new_global_ref(vpn_service) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("Failed to create global ref for VpnService: {:?}", e);
            return;
        }
    };
    let callback: Box<dyn Fn(i32) -> bool + Send + Sync> = Box::new(move |fd: i32| {
        call_vpn_protect(&global, fd)
    });
    crate::network::set_protect_callback(callback);
    tracing::info!("Protect callback set");
}

#[cfg(target_os = "android")]
fn call_vpn_protect(vpn_service: &GlobalRef, fd: i32) -> bool {
    use ::jni::objects::JValue;
    let vm = match JNI_JVM.get() {
        Some(vm) => vm,
        None => {
            tracing::error!("No JVM available for protect call");
            return false;
        }
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            tracing::error!("Failed to attach thread for protect: {:?}", e);
            return false;
        }
    };
    // Call VpnService.protect(int) : boolean
    let service = vpn_service.as_obj();
    match env.call_method(service, "protect", "(I)Z", &[JValue::Int(fd)]) {
        Ok(val) => val.z().unwrap_or(false),
        Err(e) => {
            tracing::error!("VpnService.protect() failed: {:?}", e);
            false
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeClearProtectCallback(
    _env: JNIEnv,
    _class: JClass,
) {
    crate::network::clear_protect_callback();
    tracing::info!("Protect callback cleared");
}

// === Credentials ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSaveCredential(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    cred_type: JString,
    value: JString,
) -> jboolean {
    let sid = jstring_to_string(&mut env, &server_id);
    let ct = jstring_to_string(&mut env, &cred_type);
    let val = jstring_to_string(&mut env, &value);
    let store = crate::credential::android_credential_store();
    let key = termfast_credential::make_key(&sid, &ct);
    bool_to_jbool(store.save(&key, &val).is_ok())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeLoadCredential(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    cred_type: JString,
) -> jstring {
    let sid = jstring_to_string(&mut env, &server_id);
    let ct = jstring_to_string(&mut env, &cred_type);
    let store = crate::credential::android_credential_store();
    let key = termfast_credential::make_key(&sid, &ct);
    match store.load(&key) {
        Ok(val) => string_to_jstring(&mut env, &val).into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeDeleteCredential(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    cred_type: JString,
) -> jboolean {
    let sid = jstring_to_string(&mut env, &server_id);
    let ct = jstring_to_string(&mut env, &cred_type);
    let store = crate::credential::android_credential_store();
    let key = termfast_credential::make_key(&sid, &ct);
    bool_to_jbool(store.delete(&key).is_ok())
}

// === Credential encryption management ===

/// Returns one of: "needs_setup", "needs_migration", "locked", "unlocked"
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let store = crate::credential::android_credential_store();
    let status = if store.is_pending() {
        "pending"
    } else if store.is_legacy_plaintext() {
        "needs_migration"
    } else if store.is_unlocked() {
        "unlocked"
    } else {
        "locked"
    };
    string_to_jstring(&mut env, status).into_raw()
}

/// Initialize a new encrypted credential store with the given master password.
/// Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialInitialize(
    mut env: JNIEnv,
    _class: JClass,
    master_password: JString,
) -> jboolean {
    let pw = jstring_to_secret(&mut env, &master_password);
    let store = crate::credential::android_credential_store();
    let ok = store.initialize(&pw).is_ok();
    if ok {
        // Clear stale sync password hash — new master password means
        // the old cloud sync password hash is no longer relevant.
        let _ = std::fs::remove_file(crate::cloud_sync::data_dir().join("sync_hash.dat"));
    }
    bool_to_jbool(ok)
}

/// Unlock the credential store with a master password.
/// Returns true on success, false on error (wrong password).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialUnlock(
    mut env: JNIEnv,
    _class: JClass,
    master_password: JString,
) -> jboolean {
    let pw = jstring_to_secret(&mut env, &master_password);
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.unlock(&pw).is_ok())
}

/// Unlock using a cached derived key (raw 32 bytes, base64-encoded by Kotlin).
/// Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialUnlockWithKey(
    mut env: JNIEnv,
    _class: JClass,
    key_base64: JString,
) -> jboolean {
    let encoded = jstring_to_string(&mut env, &key_base64);
    let bytes = match base64_decode(&encoded) {
        Some(b) if b.len() == 32 => b,
        _ => return false as jboolean,
    };
    let key = termfast_credential::DerivedKey::from_bytes(&bytes);
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.unlock_with_key(key).is_ok())
}

/// Get the derived key (base64-encoded 32 bytes) for caching in Android Keystore.
/// Returns null if the store is locked.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialGetKey(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let store = crate::credential::android_credential_store();
    match store.derived_key() {
        Some(key) => {
            let encoded = base64_encode(key.as_bytes());
            string_to_jstring(&mut env, &encoded).into_raw()
        }
        None => std::ptr::null_mut(),
    }
}

/// Lock the credential store (clear cached key and map from memory).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialLock(
    _env: JNIEnv,
    _class: JClass,
) {
    let store = crate::credential::android_credential_store();
    store.lock();
}

/// Migrate a legacy plaintext credential file to encrypted format.
/// Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialMigrate(
    mut env: JNIEnv,
    _class: JClass,
    master_password: JString,
) -> jboolean {
    let pw = jstring_to_secret(&mut env, &master_password);
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.migrate(&pw).is_ok())
}

/// Change the master password. Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialChangePassword(
    mut env: JNIEnv,
    _class: JClass,
    old_password: JString,
    new_password: JString,
) -> jboolean {
    let old = jstring_to_string(&mut env, &old_password);
    let new = jstring_to_string(&mut env, &new_password);
    let store = crate::credential::android_credential_store();
    let ok = store.change_password(&old, &new).is_ok();
    if ok {
        // Clear stale sync password hash — password changed, old cloud
        // sync password hash no longer matches.
        let _ = std::fs::remove_file(crate::cloud_sync::data_dir().join("sync_hash.dat"));
    }
    bool_to_jbool(ok)
}

/// Reset (delete) the encrypted credential file and lock.
/// Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialReset(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let store = crate::credential::android_credential_store();
    let ok = store.reset().is_ok();
    if ok {
        // Clear sync password hash on reset too.
        let _ = std::fs::remove_file(crate::cloud_sync::data_dir().join("sync_hash.dat"));
    }
    bool_to_jbool(ok)
}

/// Export the encrypted credential file to a destination path.
/// Returns true on success, false on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialExport(
    mut env: JNIEnv,
    _class: JClass,
    dest_path: JString,
) -> jboolean {
    let dest = jstring_to_string(&mut env, &dest_path);
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.export_to(std::path::Path::new(&dest)).is_ok())
}

/// Import an encrypted credential file from a source path.
/// Verifies the master password before overwriting. Returns true on success.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialImport(
    mut env: JNIEnv,
    _class: JClass,
    src_path: JString,
    master_password: JString,
) -> jboolean {
    let src = jstring_to_string(&mut env, &src_path);
    let pw = jstring_to_secret(&mut env, &master_password);
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.import_from(std::path::Path::new(&src), &pw).is_ok())
}

/// Check if the credential store is currently unlocked.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCredentialIsUnlocked(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let store = crate::credential::android_credential_store();
    bool_to_jbool(store.is_unlocked())
}

// --- base64 helpers (no external dep on Android) ---

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len() * 4 / 3 + 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push(TABLE[((n >> 6) & 63) as usize] as char);
        s.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push_str("==");
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push(TABLE[((n >> 6) & 63) as usize] as char);
        s.push('=');
    }
    s
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf = Vec::with_capacity(s.len() * 3 / 4);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let mut vals = [0u8; 4];
        let mut pad = 0;
        for j in 0..4 {
            let b = bytes[i + j];
            if b == b'=' {
                vals[j] = 0;
                pad += 1;
            } else {
                vals[j] = TABLE.iter().position(|&c| c == b)? as u8;
            }
        }
        let n = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);
        buf.push((n >> 16) as u8);
        if pad < 2 {
            buf.push((n >> 8) as u8);
        }
        if pad < 1 {
            buf.push(n as u8);
        }
        i += 4;
    }
    Some(buf)
}

// === Key generation ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeGenerateKeypair(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
) -> jstring {
    let sid = jstring_to_string(&mut env, &server_id);
    let st = state().lock().unwrap();
    let data_dir = st.data_dir.clone();
    let key_dir = std::path::PathBuf::from(&data_dir).join("keys").join(&sid);
    let _ = std::fs::create_dir_all(&key_dir);
    match termfast_core::ssh::auth::generate_keypair_at(&key_dir, &sid) {
        Ok((key_path, public_key, passphrase)) => {
            let private_key = std::fs::read_to_string(&key_path).unwrap_or_default();
            let json = serde_json::json!({
                "private_key": private_key,
                "public_key": public_key,
                "passphrase": passphrase,
                "key_path": key_path.to_string_lossy(),
            }).to_string();
            string_to_jstring(&mut env, &json).into_raw()
        }
        Err(e) => {
            tracing::error!("Key generation failed: {:?}", e);
            string_to_jstring(&mut env, "{}").into_raw()
        }
    }
}

// === Triggers ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeListTriggers(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
) -> jstring {
    let sid = jstring_to_string(&mut env, &server_id);
    let st = state().lock().unwrap();
    let triggers = if let Some(ref cm) = st.config_manager {
        let cfg = cm.get_blocking();
        cfg.find_server(&sid).map(|s| s.triggers.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let json = serde_json::to_string(&triggers).unwrap_or_default();
    string_to_jstring(&mut env, &json).into_raw()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeListTriggerTemplates(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let st = state().lock().unwrap();
    let templates = if let Some(ref cm) = st.config_manager {
        let cfg = cm.get_blocking();
        cfg.trigger_templates.clone()
    } else {
        Vec::new()
    };
    let json = serde_json::to_string(&templates).unwrap_or_default();
    string_to_jstring(&mut env, &json).into_raw()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeSetServerTriggers(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    json: JString,
) -> jboolean {
    let sid = jstring_to_string(&mut env, &server_id);
    let json_str = jstring_to_string(&mut env, &json);
    let triggers: Vec<TriggerInstance> = match serde_json::from_str(&json_str) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to parse triggers: {:?}", e);
            return bool_to_jbool(false);
        }
    };
    let (cm, instance) = {
        let st = state().lock().unwrap();
        (st.config_manager.clone(), st.servers.get(&sid).cloned())
    };
    if let Some(ref cm) = cm {
        let rt = runtime();
        let _ = rt.block_on(cm.modify(|cfg| {
            if let Some(server) = cfg.find_server_mut(&sid) {
                server.triggers = triggers.clone();
            }
        }));
    }
    if let Some(instance) = instance {
        let rt = runtime();
        let _ = rt.block_on(instance.set_triggers(triggers));
    }
    bool_to_jbool(true)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeRunTrigger(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    trigger_id: JString,
) -> jstring {
    let sid = jstring_to_string(&mut env, &server_id);
    let tid = jstring_to_string(&mut env, &trigger_id);
    let st = state().lock().unwrap();
    let result = if let Some(instance) = st.servers.get(&sid).cloned() {
        let rt = runtime();
        // Check if SSH is connected before running trigger
        let connected = rt.block_on(instance.ssh_client.is_connected());
        if !connected {
            Err(termfast_core::error::Error::Other(
                "SSH 未连接，请先启动 VPN 或代理".to_string(),
            ))
        } else {
            rt.block_on(instance.manual_fire_trigger(&tid))
        }
    } else {
        Err(termfast_core::error::Error::Other(format!("server {} not found", sid)))
    };
    let json = match result {
        Ok(r) => {
            serde_json::json!({
                "success": r.success,
                "trigger_id": r.trigger_id,
                "trigger_name": r.trigger_name,
                "executed_commands": r.executed_commands,
                "total_commands": r.total_commands,
                "results": r.results.iter().map(|cmd| serde_json::json!({
                    "command": cmd.command,
                    "exit_code": cmd.exit_code,
                    "stdout": cmd.stdout,
                    "stderr": cmd.stderr,
                    "success": cmd.success,
                })).collect::<Vec<_>>(),
                "error": null,
            }).to_string()
        }
        Err(e) => serde_json::json!({"success": false, "error": e.to_string()}).to_string(),
    };
    string_to_jstring(&mut env, &json).into_raw()
}

// === SSH Terminal (PTY) ===

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeOpenTerminal(
    mut env: JNIEnv,
    _class: JClass,
    server_id: JString,
    session_id: JString,
    cols: jint,
    rows: jint,
) -> jboolean {
    let sid = jstring_to_string(&mut env, &server_id);
    let session = jstring_to_string(&mut env, &session_id);
    let cols = if cols > 0 { cols as u32 } else { 80 };
    let rows = if rows > 0 { rows as u32 } else { 24 };

    let rt = runtime();
    let result = rt.block_on(crate::pty_api::open_session(&sid, &session, cols, rows));
    match result {
        Ok(()) => {
            log_to_kotlin("info", &format!("Terminal opened for server {}", sid));
            bool_to_jbool(true)
        }
        Err(e) => {
            log_to_kotlin("error", &format!("Failed to open terminal: {}", e));
            // Send error event to Kotlin
            let json = serde_json::json!({
                "type": "TerminalError",
                "session_id": session,
                "error": e,
            });
            dispatch_event_to_kotlin(&json.to_string());
            bool_to_jbool(false)
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeWriteTerminal(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    data: JString,
) -> jboolean {
    let session = jstring_to_string(&mut env, &session_id);
    let input = jstring_to_string(&mut env, &data);
    match crate::pty_api::write_session(&session, input.as_bytes()) {
        Ok(()) => bool_to_jbool(true),
        Err(e) => {
            log_to_kotlin("error", &format!("Terminal write error: {}", e));
            bool_to_jbool(false)
        }
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloseTerminal(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
) -> jboolean {
    let session = jstring_to_string(&mut env, &session_id);
    crate::pty_api::close_session(&session);
    bool_to_jbool(true)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeResizeTerminal(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    cols: jint,
    rows: jint,
) -> jboolean {
    let session = jstring_to_string(&mut env, &session_id);
    let cols = if cols > 0 { cols as u32 } else { 80 };
    let rows = if rows > 0 { rows as u32 } else { 24 };
    let rt = runtime();
    let _ = rt.block_on(crate::pty_api::resize_session(&session, cols, rows));
    bool_to_jbool(true)
}

// === Cloud Sync ===

/// Get the OAuth authorization URL for a cloud provider.
/// Returns a JSON string `{"auth_url":"...","provider":"..."}` or empty string on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncAuthUrl(
    mut env: JNIEnv,
    _class: JClass,
    provider: JString,
) -> jstring {
    let prov = jstring_to_string(&mut env, &provider);
    match crate::cloud_sync::auth_url(&prov) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(e) => {
            log::error!("cloud_sync_auth_url: {}", e);
            string_to_jstring(&mut env, "").into_raw()
        }
    }
}

/// Exchange an OAuth code for a token.
/// `code` comes from the deep-link callback.
/// Returns a JSON string with token fields, or empty string on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncExchangeCode(
    mut env: JNIEnv,
    _class: JClass,
    code: JString,
) -> jstring {
    let code = jstring_to_string(&mut env, &code);
    match crate::cloud_sync::exchange_code(&code) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(e) => {
            log::error!("cloud_sync_exchange_code: {}", e);
            string_to_jstring(&mut env, "").into_raw()
        }
    }
}

/// Save a token to the token store.
/// `token_json` is the JSON returned by `nativeCloudSyncExchangeCode`.
/// Returns true on success.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncSaveToken(
    mut env: JNIEnv,
    _class: JClass,
    token_json: JString,
) -> jboolean {
    let json = jstring_to_string(&mut env, &token_json);
    bool_to_jbool(crate::cloud_sync::save_token(&json).is_ok())
}

/// Check if a provider is authenticated.
/// Returns JSON `{"authenticated":true,"expires_at":...}` or `{"authenticated":false}`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncLoadToken(
    mut env: JNIEnv,
    _class: JClass,
    provider: JString,
) -> jstring {
    let prov = jstring_to_string(&mut env, &provider);
    match crate::cloud_sync::load_token(&prov) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(_) => string_to_jstring(&mut env, r#"{"authenticated":false}"#).into_raw(),
    }
}

/// Upload encrypted config to cloud.
/// `params_json`: `{"provider":"dropbox","master_password":"xxx","force":false}`
/// Returns JSON with `ok`/`conflict`/`reason` fields, or empty string on error.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncUpload(
    mut env: JNIEnv,
    _class: JClass,
    params_json: JString,
) -> jstring {
    let params = jstring_to_string(&mut env, &params_json);
    match crate::cloud_sync::upload(&params) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(e) => {
            log::error!("cloud_sync_upload: {}", e);
            string_to_jstring(&mut env, &serde_json::json!({"ok":false,"reason":"error","message":e}).to_string()).into_raw()
        }
    }
}

/// Download and apply encrypted config from cloud.
/// `params_json`: `{"provider":"dropbox","master_password":"xxx","force_download":false}`
/// Returns JSON with `ok`/`reason`/`device_name`/`updated_at`/`size` fields.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncDownload(
    mut env: JNIEnv,
    _class: JClass,
    params_json: JString,
) -> jstring {
    let params = jstring_to_string(&mut env, &params_json);
    match crate::cloud_sync::download(&params) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(e) => {
            log::error!("cloud_sync_download: {}", e);
            string_to_jstring(&mut env, &serde_json::json!({"ok":false,"reason":"error","message":e}).to_string()).into_raw()
        }
    }
}

/// Get cloud sync status for a provider.
/// Returns JSON with `authenticated`/`has_remote`/`remote_size`/`last_synced` fields.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncStatus(
    mut env: JNIEnv,
    _class: JClass,
    provider: JString,
) -> jstring {
    let prov = jstring_to_string(&mut env, &provider);
    match crate::cloud_sync::status(&prov) {
        Ok(json) => string_to_jstring(&mut env, &json).into_raw(),
        Err(_) => string_to_jstring(&mut env, r#"{"authenticated":false,"has_remote":false}"#).into_raw(),
    }
}

/// Disconnect a provider (remove its token).
/// Returns true on success.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_termfast_app_RustBridge_nativeCloudSyncDisconnect(
    mut env: JNIEnv,
    _class: JClass,
    provider: JString,
) -> jboolean {
    let prov = jstring_to_string(&mut env, &provider);
    bool_to_jbool(crate::cloud_sync::disconnect(&prov).is_ok())
}
