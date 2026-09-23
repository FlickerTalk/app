mod client;
mod plugins;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_ft_platform::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    builder
        .manage(client::Client::default())
        .manage(plugins::Plugins::default())
        // Each plugin is served from its own folder, inside an iframe, with the policy its
        // permissions allow (issue app#3, §55, §58).
        .register_uri_scheme_protocol("ftplugin", |ctx, request| {
            let served = request.uri().path().to_owned();
            let answer = plugins::route(&served)
                .and_then(|(id, file)| ctx.app_handle().state::<plugins::Plugins>().get(&id).map(|one| (one, file)))
                .and_then(|(one, file)| {
                    // The app lends its icons to every plugin: `./icon/<name>.svg` (§53).
                    if let Some(name) = file.strip_prefix("icon/").and_then(|name| name.strip_suffix(".svg")) {
                        let (kind, svg) = plugins::icon(name)?;
                        return Some((svg.to_vec(), kind, one.policy));
                    }
                    let (body, kind) = if file == "frame.html" {
                        (plugins::frame_html(&one.component).into_bytes(), plugins::content_type("frame.html"))
                    } else if file == "frame.js" {
                        (plugins::frame_js().as_bytes().to_vec(), plugins::content_type("frame.js"))
                    } else {
                        let path = plugins::file_in(&one.dir, &file)?;
                        (std::fs::read(path).ok()?, plugins::content_type(&file))
                    };
                    Some((body, kind, one.policy))
                });
            match answer {
                Some((body, kind, policy)) => tauri::http::Response::builder()
                    .header(tauri::http::header::CONTENT_TYPE, kind)
                    .header(tauri::http::header::CONTENT_SECURITY_POLICY, policy)
                    .header(plugins::ALLOW_OPAQUE_ORIGIN.0, plugins::ALLOW_OPAQUE_ORIGIN.1)
                    .header("Cross-Origin-Resource-Policy", "cross-origin")
                    .body(body)
                    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
                None => tauri::http::Response::builder()
                    .status(tauri::http::StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
            }
        })
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
            client::core_pending_call,
            client::core_pick_files,
            client::core_read_picked,
            client::core_send_made,
            client::core_send_picked,
            client::core_plugins,
            client::core_plugin_grant,
            client::core_plugin_remove,
            client::core_forget_message,
            client::core_forward,
            client::core_share_message,
            client::core_catalogue,
            client::core_plugin_add,
            client::core_plugin_read,
            client::core_plugin_write,
            client::core_plugin_forget,
            client::core_plugin_fetch,
            client::core_plugin_save,
            client::core_plugin_print,
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
