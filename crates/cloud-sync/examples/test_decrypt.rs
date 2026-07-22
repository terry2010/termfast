use termfast_cloud_sync::sync_crypto;
fn main() {
    let payload = sync_crypto::SyncPayload {
        config: serde_json::json!({"test": "data"}),
        device_name: "test".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let blob = sync_crypto::encrypt_config("correctPassword", &payload).unwrap();
    println!("Blob version byte: {}", blob[4]);
    // Test wrong password
    match sync_crypto::decrypt_config("wrongPassword", &blob) {
        Ok(_) => println!("BUG: wrong password decrypted successfully!"),
        Err(e) => println!("Correct: wrong password rejected: {}", e),
    }
    // Test correct password
    match sync_crypto::decrypt_config("correctPassword", &blob) {
        Ok(p) => println!("Correct: right password works, device={}", p.device_name),
        Err(e) => println!("BUG: right password failed: {}", e),
    }
}
