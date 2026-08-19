// src-tauri/src/pairing.rs — Desktop pairing IPC helpers
use reqwest::Client;
use serde_json::Value;

const BACKEND_URL: &str = "http://sh.zimufan.com:39527";

fn client() -> Client {
    Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap()
}

pub async fn auth_register(email: &str, password: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/auth/register", BACKEND_URL))
        .json(&serde_json::json!({"email": email, "password": password}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error").to_string());
    }
    Ok(body)
}

pub async fn auth_login(email: &str, password: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/auth/login", BACKEND_URL))
        .json(&serde_json::json!({"email": email, "password": password}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("invalid credentials").to_string());
    }
    Ok(body)
}

/// Refresh the user access token using a refresh token.
/// Returns { "access_token": "...", "token_type": "Bearer" }
pub async fn auth_refresh(refresh_token: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/auth/refresh", BACKEND_URL))
        .json(&serde_json::json!({"refresh_token": refresh_token}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("refresh failed").to_string());
    }
    Ok(body)
}

/// Build the JSON body for pair_initiate (extracted for testing).
pub fn build_initiate_body(
    desktop_device_id: &str,
    desktop_name: &str,
    device_public_key: &str,
    key_security_level: &str,
) -> Value {
    serde_json::json!({
        "desktop_device_id": desktop_device_id,
        "desktop_name": desktop_name,
        "device_public_key": device_public_key,
        "key_security_level": key_security_level,
    })
}

pub async fn pair_initiate(
    token: &str,
    desktop_device_id: &str,
    desktop_name: &str,
    device_public_key: &str,
    key_security_level: &str,
) -> Result<Value, String> {
    let body = build_initiate_body(
        desktop_device_id,
        desktop_name,
        device_public_key,
        key_security_level,
    );
    let resp = client()
        .post(format!("{}/pair/initiate", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

pub async fn pair_status(token: &str, pairing_id: &str) -> Result<Value, String> {
    let resp = client()
        .get(format!("{}/pair/status?pairing_id={}", BACKEND_URL, pairing_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

pub async fn pair_revoke(token: &str, pairing_id: &str) -> Result<Value, String> {
    let resp = client()
        .delete(format!("{}/pair/{}", BACKEND_URL, pairing_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

/// Re-issue a pairing JWT for an already-completed desktop pairing.
/// Used by desktop clients that lost their local pairing JWT.
pub async fn reissue_pairing_jwt(token: &str, pairing_id: &str) -> Result<String, String> {
    let resp = client()
        .post(format!("{}/pair/reissue-jwt", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "pairing_id": pairing_id }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    body.get("pairing_jwt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "missing pairing_jwt in response".to_string())
}

pub async fn sync_upload_config(pairing_jwt: &str, ciphertext: &str, nonce: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/sync/config", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", pairing_jwt))
        .json(&serde_json::json!({"ciphertext": ciphertext, "nonce": nonce}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

pub async fn list_devices(token: &str, desktop_device_id: &str) -> Result<Value, String> {
    let url = if desktop_device_id.is_empty() {
        format!("{}/devices", BACKEND_URL)
    } else {
        format!("{}/devices?desktop_device_id={}", BACKEND_URL, desktop_device_id)
    };
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

pub async fn send_push(
    token: &str,
    pairing_id: &str,
    event_type: &str,
    title: &str,
    body: &str,
    terminal_id: Option<&str>,
) -> Result<Value, String> {
    let mut payload = serde_json::json!({
        "pairing_id": pairing_id,
        "event_type": event_type,
        "title": title,
        "body": body,
    });
    if let Some(tid) = terminal_id {
        payload["terminal_id"] = serde_json::Value::String(tid.to_string());
    }
    let resp = client()
        .post(format!("{}/push", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

/// D4: Submit ApproveJoin to backend (POST /join/approve).
pub async fn approve_join(
    token: &str,
    batch_id: &str,
    approver_device_id: &str,
    payload: &str,
    signature: &str,
) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/join/approve", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "batch_id": batch_id,
            "approver_device_id": approver_device_id,
            "payload": payload,
            "signature": signature,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

/// D8: Get batch info from backend (GET /join/batch-info).
pub async fn get_batch_info(
    token: &str,
    batch_id: &str,
    device_id: &str,
) -> Result<Value, String> {
    let resp = client()
        .get(format!(
            "{}/join/batch-info?batch_id={}&device_id={}",
            BACKEND_URL, batch_id, device_id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}

/// Initiate a desktop-to-desktop pairing (FP-1 backend API).
pub async fn pair_initiate_desktop(
    token: &str,
    server_user_id: u64,
    server_device_id: &str,
    server_name: &str,
    client_user_id: u64,
    client_device_id: &str,
    client_name: &str,
) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/pair/initiate-desktop", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "server_user_id": server_user_id,
            "server_device_id": server_device_id,
            "server_name": server_name,
            "client_user_id": client_user_id,
            "client_device_id": client_device_id,
            "client_name": client_name,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("error").to_string());
    }
    Ok(body)
}
