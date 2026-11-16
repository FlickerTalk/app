use super::*;
use crate::package::fixtures::{manifest_of, package};
use crate::package::open;
use vodozemac::Ed25519SecretKey;

fn manifest_with(permissions: &str) -> String {
    format!(
        r#"{{"id":"com.example.ai","name":"Assistant","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-ai"],"permissions":{permissions}}}"#
    )
}

#[test]
fn a_plugin_asks_for_what_it_needs_and_nothing_else() {
    let catalogue = Ed25519SecretKey::new();
    let plain = open(
        &package(&manifest_of("com.example.code"), b"", &catalogue),
        &catalogue.public_key(),
    )
    .unwrap();
    assert_eq!(plain.manifest.permissions, Permissions::default());
    assert!(
        plain.manifest.permissions.network.is_empty(),
        "no network unless it asks"
    );
    assert_eq!(plain.manifest.permissions.send, Sending::Nothing);

    let asking =
        manifest_with(r#"{"messages":"given","send":"propose","network":["api.openai.com"]}"#);
    let ai = open(&package(&asking, b"", &catalogue), &catalogue.public_key()).unwrap();
    assert_eq!(ai.manifest.permissions.network, ["api.openai.com"]);
    assert_eq!(ai.manifest.permissions.send, Sending::Propose);
    assert!(ai.manifest.permissions.reads_given_messages);
    assert!(
        !ai.manifest.permissions.print,
        "printing is asked for on its own"
    );

    let printer = manifest_with(r#"{"print":true}"#);
    let opened = open(&package(&printer, b"", &catalogue), &catalogue.public_key()).unwrap();
    assert!(opened.manifest.permissions.print);
    assert!(opened.manifest.permissions.network.is_empty());
}

#[test]
fn a_plugin_asks_for_the_phones_position_on_its_own() {
    let catalogue = Ed25519SecretKey::new();
    let asking = open(
        &package(
            &manifest_with(r#"{"location":true,"send":"propose"}"#),
            b"",
            &catalogue,
        ),
        &catalogue.public_key(),
    )
    .unwrap();
    let said = serde_json::to_value(&asking.manifest.permissions).unwrap();
    assert_eq!(
        said["location"],
        serde_json::json!(true),
        "the manifest asked for the position: {said}"
    );
    let plain = open(
        &package(&manifest_of("com.example.code"), b"", &catalogue),
        &catalogue.public_key(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&plain.manifest.permissions).unwrap()["location"],
        serde_json::json!(false),
        "never unless asked"
    );
    for wrong in [
        r#"{"location":"always"}"#,
        r#"{"location":"background"}"#,
        r#"{"location":1}"#,
    ] {
        assert!(
            open(
                &package(&manifest_with(wrong), b"", &catalogue),
                &catalogue.public_key()
            )
            .is_err(),
            "{wrong} is not a yes or a no"
        );
    }
    let before: Permissions =
        serde_json::from_str(r#"{"network":[],"messages":"none","send":"nothing"}"#).unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap()["location"],
        serde_json::json!(false)
    );
}

#[test]
fn a_plugin_asks_to_translate_on_its_own() {
    let catalogue = Ed25519SecretKey::new();
    let asking = open(
        &package(
            &manifest_with(r#"{"translate":true,"messages":"given","send":"propose"}"#),
            b"",
            &catalogue,
        ),
        &catalogue.public_key(),
    )
    .unwrap();
    let said = serde_json::to_value(&asking.manifest.permissions).unwrap();
    assert_eq!(
        said["translate"],
        serde_json::json!(true),
        "the manifest asked for the translator: {said}"
    );
    let plain = open(
        &package(&manifest_of("com.example.code"), b"", &catalogue),
        &catalogue.public_key(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&plain.manifest.permissions).unwrap()["translate"],
        serde_json::json!(false),
        "never unless asked"
    );
    for wrong in [
        r#"{"translate":"always"}"#,
        r#"{"translate":1}"#,
        r#"{"translate":"cloud"}"#,
    ] {
        assert!(
            open(
                &package(&manifest_with(wrong), b"", &catalogue),
                &catalogue.public_key()
            )
            .is_err(),
            "{wrong} is not a yes or a no"
        );
    }
    let before: Permissions =
        serde_json::from_str(r#"{"network":[],"messages":"none","send":"nothing"}"#).unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap()["translate"],
        serde_json::json!(false)
    );
}

#[test]
fn a_plugin_asks_for_the_live_channel_reminders_the_drive_and_room_on_their_own() {
    let catalogue = Ed25519SecretKey::new();
    let asking = manifest_with(r#"{"live":true,"remind":true,"drive":true,"storage":"large"}"#);
    let opened = open(&package(&asking, b"", &catalogue), &catalogue.public_key()).unwrap();
    let permissions = &opened.manifest.permissions;
    assert!(permissions.live && permissions.remind && permissions.drive);
    assert_eq!(permissions.storage, Storage::Large);
    assert!(Storage::Large.quota() > Storage::Small.quota());
    let plain = open(
        &package(&manifest_of("com.example.code"), b"", &catalogue),
        &catalogue.public_key(),
    )
    .unwrap();
    assert!(
        !plain.manifest.permissions.live
            && !plain.manifest.permissions.remind
            && !plain.manifest.permissions.drive
    );
    assert_eq!(plain.manifest.permissions.storage, Storage::Small);
    assert!(open(
        &package(&manifest_with(r#"{"storage":"huge"}"#), b"", &catalogue),
        &catalogue.public_key()
    )
    .is_err());

    let opens = r#"{"id":"com.example.board","name":"Board","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-board"],"opens":["application/x-ftboard","image/*"]}"#;
    let board = open(&package(opens, b"", &catalogue), &catalogue.public_key()).unwrap();
    assert!(board.manifest.opens_kind("application/x-ftboard"));
    assert!(board.manifest.opens_kind("image/png"));
    assert!(board.manifest.opens_kind("IMAGE/JPEG; charset=x"));
    assert!(!board.manifest.opens_kind("video/mp4"));
    assert!(
        !plain.manifest.opens_kind("image/png"),
        "it opens nothing unless it says so"
    );
    let viewer = r#"{"id":"com.example.pdf","name":"PDF","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-pdf"],"opens":["application/pdf"],"views":["application/pdf"]}"#;
    let pdf = open(&package(viewer, b"", &catalogue), &catalogue.public_key()).unwrap();
    assert!(pdf.manifest.views_kind("application/pdf"));
    assert!(pdf.manifest.views_kind("Application/PDF; charset=x"));
    assert!(!pdf.manifest.views_kind("image/png"));
    assert!(
        !board.manifest.views_kind("image/png"),
        "opening is not viewing"
    );
    for wrong in [
        r#""opens":["application/pdf"],"views":["application/*"]"#,
        r#""opens":["application/pdf"],"views":["*/*"]"#,
        r#""opens":["image/*"],"views":["application/pdf"]"#,
        r#""opens":["application/pdf"],"views":["application/pdf"],"permissions":{"network":["api.example.com"]}"#,
    ] {
        let bad = format!(
            r#"{{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],{wrong}}}"#
        );
        assert!(
            open(&package(&bad, b"", &catalogue), &catalogue.public_key()).is_err(),
            "{wrong}"
        );
    }
    let any = r#"{"id":"com.example.drive","name":"Drive","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-drive"],"opens":["*/*"]}"#;
    assert!(
        open(&package(any, b"", &catalogue), &catalogue.public_key())
            .unwrap()
            .manifest
            .opens_kind("video/mp4")
    );
    for wrong in ["png", "image/", "*/png", "image/*; q=1", "../x"] {
        let bad = format!(
            r#"{{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"opens":["{wrong}"]}}"#
        );
        assert!(
            open(&package(&bad, b"", &catalogue), &catalogue.public_key()).is_err(),
            "{wrong} should be refused"
        );
    }
}

#[test]
fn a_plugin_cannot_ask_for_the_whole_internet() {
    let catalogue = Ed25519SecretKey::new();
    for asked in [
        r#"{"network":["*"]}"#,
        r#"{"network":["*.com"]}"#,
        r#"{"network":["https://api.openai.com"]}"#,
        r#"{"network":["api.openai.com/v1/chat"]}"#,
        r#"{"network":["API.OPENAI.COM"]}"#,
        r#"{"network":[""]}"#,
    ] {
        let bytes = package(&manifest_with(asked), b"", &catalogue);
        assert!(
            open(&bytes, &catalogue.public_key()).is_err(),
            "{asked} should be refused"
        );
    }
}

#[test]
fn the_content_security_policy_matches_what_was_granted() {
    let none = Permissions::default();
    assert!(none
        .content_security_policy()
        .contains("connect-src 'none'"));
    assert!(none
        .content_security_policy()
        .starts_with("default-src 'none'"));
    let policy = none.content_security_policy();
    assert!(
        policy.contains("script-src http://ftplugin.localhost"),
        "{policy}"
    );
    assert!(policy.contains("ftplugin://localhost"), "{policy}");
    assert!(!policy.contains("script-src 'self'"), "{policy}");
    assert!(
        policy.contains("frame-ancestors http://tauri.localhost tauri://localhost"),
        "{policy}"
    );
    assert!(policy.contains("child-src 'none'"), "{policy}");
    assert!(
        policy.contains("img-src") && policy.contains("data: blob:"),
        "{policy}"
    );
    assert!(
        !policy.contains("frame-ancestors 'none'"),
        "that would keep the app from showing it"
    );

    let ai = Permissions {
        network: vec!["api.openai.com".to_owned()],
        ..Permissions::default()
    };
    assert!(ai
        .content_security_policy()
        .contains("connect-src https://api.openai.com"));
    assert!(!ai.content_security_policy().contains("'unsafe-eval'"));
}
