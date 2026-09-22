mod client;
mod poc;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    builder
        .manage(poc::Poc::default())
        .manage(client::Client::default())
        .setup(|app| {
            app.state::<client::Client>().setup(app.handle())?;
            client::start_in_background(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            client::core_me,
            client::core_set_name,
            client::core_set_mailbox,
            client::core_card,
            client::core_add_contact,
            client::core_conversations,
            client::core_messages,
            client::core_send,
            client::core_mark_read,
            client::core_contact,
            client::core_block,
            client::core_rename,
            poc::poc_default_relay,
            poc::poc_connect,
            poc::poc_send
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
