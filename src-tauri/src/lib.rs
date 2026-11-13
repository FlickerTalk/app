mod client;
mod core_events;
mod opened_links;
mod plugins;
mod push_core;
mod support;
mod video_surfaces;

#[cfg(test)]
#[path = "../google_client_check.rs"]
mod google_client_check;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ft_push::initialize_tls();
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_ft_platform::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    // Start the core from a plugin instead of setup(). A VoIP push launches the iPhone app with no
    // scene, and Tauri only runs setup once there is one, too late for the call's offer.
    let builder = builder.plugin(
        tauri::plugin::Builder::<_, ()>::new("ft-boot")
            .setup(|app, _| {
                app.state::<client::Client>().setup(app)?;
                client::start_in_background(app);
                opened_links::listen(app);
                Ok(())
            })
            .build(),
    );
    builder
        .manage(client::Client::default())
        .manage(plugins::Plugins::default())
        .manage(client::plugins::OpenPlugins::default())
        .register_uri_scheme_protocol("ftplugin", |ctx, request| {
            let served = request.uri().path().to_owned();
            let answer = plugins::route(&served)
                .and_then(|(id, file)| ctx.app_handle().state::<plugins::Plugins>().get(&id).map(|one| (one, file)))
                .and_then(|(one, file)| {
                    if let Some(name) = file.strip_prefix("icon/").and_then(|name| name.strip_suffix(".svg")) {
                        let (kind, svg) = plugins::icon(name)?;
                        return Some((svg.to_vec(), kind, one.policy));
                    }
                    if let Some(name) = file.strip_prefix("ionic/") {
                        let (kind, bytes) = plugins::ionic(name)?;
                        return Some((bytes.to_vec(), kind, one.policy));
                    }
                    let (body, kind) = if file == "frame.html" {
                        (plugins::frame_html(&one.component, &one.version).into_bytes(), plugins::content_type("frame.html"))
                    } else if file == "frame.js" {
                        (plugins::frame_js().as_bytes().to_vec(), plugins::content_type("frame.js"))
                    } else {
                        let path = plugins::file_in(&one.dir, &file)?;
                        (std::fs::read(path).ok()?, plugins::content_type(&file))
                    };
                    Some((body, kind, one.policy))
                });
            match answer {
                Some((body, kind, policy)) => plugins::response_headers(kind, &policy)
                    .into_iter()
                    .fold(tauri::http::Response::builder(), |answer, (name, value)| answer.header(name, value))
                    .body(body)
                    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
                None => tauri::http::Response::builder()
                    .status(tauri::http::StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
            }
        })
        .invoke_handler(tauri::generate_handler![
            client::settings::core_me,
            client::settings::core_set_name,
            client::settings::core_set_mailbox,
            client::contacts::core_card,
            client::contacts::core_add_contact,
            client::contacts::core_add_support_contact,
            client::contacts::core_support_id,
            client::chat::core_conversations,
            client::sessions::core_session_open,
            client::sessions::core_session_remove,
            client::sessions::core_session_close,
            client::sessions::core_sessions,
            client::circles::core_circles,
            client::circles::core_circle,
            client::circles::core_circle_create,
            client::circles::core_circle_invite,
            client::circles::core_circle_remove,
            client::circles::core_circle_set_admin,
            client::circles::core_circle_rename,
            client::circles::core_circle_admins_only,
            client::circles::core_circle_leave,
            client::circles::core_circle_forget,
            client::circles::core_circle_send,
            client::circles::core_circle_messages,
            client::circles::core_circle_mark_read,
            client::contacts::core_requests,
            client::contacts::core_accept_contact,
            client::contacts::core_decline_contact,
            client::contacts::core_renew_link,
            client::files::core_accept_file,
            client::settings::core_set_auto_download,
            client::chat::core_messages,
            client::chat::core_send,
            client::chat::core_resend,
            client::chat::core_mark_read,
            client::contacts::core_contact,
            client::contacts::core_block,
            client::contacts::core_rename,
            client::contacts::core_remove_contact,
            client::contacts::core_set_history,
            client::contacts::core_set_rules,
            client::chat::core_typing,
            client::chat::core_react,
            client::chat::core_pin,
            client::chat::core_edit,
            client::chat::core_search,
            client::chat::core_schedule,
            client::chat::core_delete_everyone,
            client::settings::core_set_receipts,
            client::settings::core_quiet_hours,
            client::settings::core_set_quiet_hours,
            client::files::core_upload_start,
            client::files::core_upload_append,
            client::files::core_send_file,
            client::files::core_open_file,
            client::contacts::core_share,
            client::files::core_pick_files,
            client::files::core_take_photo,
            client::files::core_pick_for_plugin,
            client::files::core_take_photo_for_plugin,
            client::plugin_api::core_plugin_made,
            client::files::core_send_picked,
            client::plugins::core_plugins,
            client::plugins::core_plugin_grant,
            client::plugins::core_plugin_remove,
            client::billing::core_plan,
            client::billing::core_subscribe,
            client::billing::core_restore_subscription,
            client::billing::core_subscription_price,
            client::settings::core_send_feedback,
            client::chat::core_forget_message,
            client::chat::core_forward,
            client::chat::core_share_message,
            client::plugins::core_catalogue,
            client::plugins::core_plugin_open,
            client::plugins::core_plugin_add,
            client::plugin_api::core_plugin_read,
            client::plugin_api::core_plugin_write,
            client::plugin_api::core_plugin_forget,
            client::plugin_api::core_plugin_record_get,
            client::plugin_api::core_plugin_record_set,
            client::plugin_api::core_plugin_record_forget,
            client::plugin_api::core_plugin_record_keys,
            client::plugin_api::core_plugin_record_usage,
            client::plugin_api::core_plugin_ref,
            client::plugin_api::core_plugin_open_chat,
            client::plugin_api::core_remind_set,
            client::plugin_api::core_remind_cancel,
            client::plugin_api::core_remind_list,
            client::plugin_api::core_pending_reminder,
            opened_links::core_opened_link,
            opened_links::core_read_link,
            client::plugin_api::core_plugin_live_send,
            client::plugin_api::core_plugin_chat,
            client::plugin_api::core_message_note_set,
            client::plugin_api::core_message_note_clear,
            client::plugin_api::core_chat_hooks,
            client::plugin_api::core_chat_hooks_set,
            client::plugin_api::core_hook_sample,
            client::plugins::core_plugins_opening,
            client::plugin_api::core_read_message_file,
            client::vault::core_vault_status,
            client::vault::core_vault_connect,
            client::vault::core_vault_setup,
            client::vault::core_vault_unlock,
            client::core_resume,
            client::vault::core_vault_suggest_phrase,
            client::vault::core_vault_change_phrase,
            client::vault::core_vault_disconnect,
            client::vault::core_vault_set_client_id,
            client::vault::core_vault_list,
            client::vault::core_vault_mkdir,
            client::vault::core_vault_rename,
            client::vault::core_vault_move,
            client::vault::core_vault_remove,
            client::vault::core_vault_upload_picked,
            client::vault::core_vault_upload_message,
            client::vault::core_vault_retry,
            client::vault::core_vault_cancel_pending,
            client::vault::core_vault_download,
            client::vault::core_vault_open,
            client::vault::core_vault_save,
            client::vault::core_vault_send,
            client::vault::core_vault_backup,
            client::vault::core_vault_backup_info,
            client::vault::core_vault_restore,
            client::vault::core_plugin_may_use_drive,
            client::plugin_api::core_plugin_fetch,
            client::plugin_api::core_plugin_save,
            client::plugin_api::core_plugin_print,
            client::plugin_api::core_plugin_location,
            client::translate::core_translate_languages,
            client::translate::core_translate_detect,
            client::translate::core_translate_status,
            client::translate::core_translate_prepare,
            client::translate::core_translate_remove,
            client::translate::core_translate_text,
            client::core_erase,
            client::files::core_save_file,
            client::calls::core_call_ice,
            client::calls::core_call_start,
            client::calls::core_call_answer,
            client::calls::core_call_end,
            client::calls::core_native_calls,
            client::calls::core_call_start_native,
            client::calls::core_call_answer_native,
            client::calls::core_call_mute,
            client::calls::core_call_present,
            client::calls::core_call_present_stop,
            client::calls::core_file_belongs_to,
            client::calls::core_call_set_video,
            client::calls::core_call_switch_camera,
            client::call_video::core_call_video_layout,
            client::calls::core_call_speaker,
            client::settings::core_system_bars,
            client::settings::core_set_appearance,
            client::plugin_api::core_exact_alarms,
            client::plugin_api::core_request_exact_alarms,
            client::calls::core_set_call_routing,
            client::calls::core_current_call,
            client::calls::core_calls,
            client::settings::core_enable_push,
            client::moving::core_move_invite,
            client::moving::core_move_to,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
