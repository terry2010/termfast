fn main() {
    // Debug: print DEP_TAURI_DEV
    let dev = std::env::var_os("DEP_TAURI_DEV");
    println!("cargo:warning=DEP_TAURI_DEV = {:?}", dev);
    tauri_build::build()
}
