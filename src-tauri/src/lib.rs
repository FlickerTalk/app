mod poc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(poc::Poc::default())
        .invoke_handler(tauri::generate_handler![poc::poc_default_relay, poc::poc_connect, poc::poc_send])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
