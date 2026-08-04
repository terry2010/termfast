//! Pairing API — HTTP client to Go backend for Android.
#![cfg(target_os = "android")]

use crate::runtime::runtime;

const BACKEND_URL: &str = "http://127.0.0.1:8443";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap()
}

pub async fn register(email: &str, password: &str) -> Result<String, String> {
    let resp = client()
        .post(format!("{}/auth/register", BACKEND_URL))
        .json(&serde_json::json!({"email": email, "password": password}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(body)
}

pub async fn login(email: &str, password: &str) -> Result<String, String> {
    let resp = client()
        .post(format!("{}/auth/login", BACKEND_URL))
        .json(&serde_json::json!({"email": email, "password": password}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(body)
}

pub async fn pair_complete(pairing_id: &str, phone_pubkey: &str, device_id: &str) -> Result<String, String> {
    let resp = client()
        .post(format!("{}/pair/complete", BACKEND_URL))
        .json(&serde_json::json!({
            "pairing_id": pairing_id,
            "phone_pubkey": phone_pubkey,
            "device_id": device_id,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(body)
}

pub async fn download_config(pairing_jwt: &str) -> Result<String, String> {
    let resp = client()
        .get(format!("{}/sync/config", BACKEND_URL))
        .header("Authorization", format!("Bearer {}", pairing_jwt))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(body)
}
