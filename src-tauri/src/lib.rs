mod client;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_ft_platform::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    builder
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
            client::core_set_history,
            client::core_upload_start,
            client::core_upload_append,
            client::core_send_file,
            client::core_open_file,
            client::core_share,
            client::core_erase,
            client::core_save_file,
            client::core_call_ice,
            client::core_call_start,
            client::core_call_answer,
            client::core_call_end,
            client::core_calls,
            client::core_enable_push,
            client::core_move_invite,
            client::core_move_to,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
