// src-tauri/src/pairing.rs — Desktop pairing IPC helpers
use reqwest::Client;
use serde_json::Value;

const BACKEND_URL: &str = "http://127.0.0.1:8443";

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

pub async fn pair_initiate(token: &str, desktop_device_id: &str) -> Result<Value, String> {
    let resp = client()
        .post(format!("{}/pair/initiate", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({"desktop_device_id": desktop_device_id}))
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

pub async fn list_devices(token: &str) -> Result<Value, String> {
    let resp = client()
        .get(format!("{}/devices", BACKEND_URL))
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
