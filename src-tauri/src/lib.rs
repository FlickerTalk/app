mod client;
mod core_events;
mod opened_links;
mod plugins;
mod push_core;
mod video_surfaces;

// The store-build check of `build.rs` (2026-09-30), tested with the app.
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
    // The core starts with the app, not with its window (2026-09-28): a call's VoIP push launches
    // the iPhone app in the background with no scene, and Tauri runs `setup` only once there is
    // one. Plugins start when the app is built, so the core reaches the router and the call's offer
    // arrives while CallKit rings.
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
        .manage(client::OpenPlugins::default())
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
                    // And its Ionic, the same for every plugin: `./ionic/<file>` (2026-10-09).
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
            client::core_me,
            client::core_set_name,
            client::core_set_mailbox,
            client::core_card,
            client::core_add_contact,
            client::core_conversations,
            client::core_session_open,
            client::core_session_remove,
            client::core_session_close,
            client::core_sessions,
            client::core_circles,
            client::core_circle,
            client::core_circle_create,
            client::core_circle_invite,
            client::core_circle_remove,
            client::core_circle_set_admin,
            client::core_circle_rename,
            client::core_circle_admins_only,
            client::core_circle_leave,
            client::core_circle_forget,
            client::core_circle_send,
            client::core_circle_messages,
            client::core_circle_mark_read,
            client::core_requests,
            client::core_accept_contact,
            client::core_decline_contact,
            client::core_renew_link,
            client::core_accept_file,
            client::core_set_auto_download,
            client::core_messages,
            client::core_send,
            client::core_resend,
            client::core_mark_read,
            client::core_contact,
            client::core_block,
            client::core_rename,
            client::core_remove_contact,
            client::core_set_history,
            client::core_set_rules,
            client::core_typing,
            client::core_react,
            client::core_pin,
            client::core_edit,
            client::core_search,
            client::core_schedule,
            client::core_delete_everyone,
            client::core_set_receipts,
            client::core_quiet_hours,
            client::core_set_quiet_hours,
            client::core_upload_start,
            client::core_upload_append,
            client::core_send_file,
            client::core_open_file,
            client::core_share,
            client::core_pick_files,
            client::core_take_photo,
            client::core_pick_for_plugin,
            client::core_take_photo_for_plugin,
            client::core_plugin_made,
            client::core_send_picked,
            client::core_plugins,
            client::core_plugin_grant,
            client::core_plugin_remove,
            client::core_plan,
            client::core_subscribe,
            client::core_restore_subscription,
            client::core_subscription_price,
            client::core_send_feedback,
            client::core_forget_message,
            client::core_forward,
            client::core_share_message,
            client::core_catalogue,
            client::core_plugin_open,
            client::core_plugin_add,
            client::core_plugin_read,
            client::core_plugin_write,
            client::core_plugin_forget,
            client::core_plugin_record_get,
            client::core_plugin_record_set,
            client::core_plugin_record_forget,
            client::core_plugin_record_keys,
            client::core_plugin_record_usage,
            client::core_plugin_ref,
            client::core_plugin_open_chat,
            client::core_remind_set,
            client::core_remind_cancel,
            client::core_remind_list,
            client::core_pending_reminder,
            opened_links::core_opened_link,
            opened_links::core_read_link,
            client::core_plugin_live_send,
            client::core_plugin_chat,
            client::core_plugins_opening,
            client::core_read_message_file,
            client::core_vault_status,
            client::core_vault_connect,
            client::core_vault_setup,
            client::core_vault_unlock,
            client::core_resume,
            client::core_vault_suggest_phrase,
            client::core_vault_change_phrase,
            client::core_vault_disconnect,
            client::core_vault_set_client_id,
            client::core_vault_list,
            client::core_vault_mkdir,
            client::core_vault_rename,
            client::core_vault_move,
            client::core_vault_remove,
            client::core_vault_upload_picked,
            client::core_vault_upload_message,
            client::core_vault_retry,
            client::core_vault_cancel_pending,
            client::core_vault_download,
            client::core_vault_open,
            client::core_vault_save,
            client::core_vault_send,
            client::core_vault_backup,
            client::core_vault_backup_info,
            client::core_vault_restore,
            client::core_plugin_may_use_drive,
            client::core_plugin_fetch,
            client::core_plugin_save,
            client::core_plugin_print,
            client::core_plugin_location,
            client::core_erase,
            client::core_save_file,
            client::core_call_ice,
            client::core_call_start,
            client::core_call_answer,
            client::core_call_end,
            client::core_native_calls,
            client::core_call_start_native,
            client::core_call_answer_native,
            client::core_call_mute,
            client::core_call_present,
            client::core_call_present_stop,
            client::core_call_set_video,
            client::core_call_switch_camera,
            client::core_call_video_layout,
            client::core_call_speaker,
            client::core_system_bars,
            client::core_set_call_routing,
            client::core_current_call,
            client::core_calls,
            client::core_enable_push,
            client::core_move_invite,
            client::core_move_to,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
