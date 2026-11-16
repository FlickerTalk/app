use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use ft_core::{Core, Peer, Transport};
use ft_plugins::{sign_package, Permissions, Player, GAMES_SINCE};
use ft_storage::Store;
use vodozemac::Ed25519SecretKey;

const CHESS: &str = "com.example.game.chess";
const DRAUGHTS: &str = "com.example.game.draughts";
const NOTES: &str = "com.example.notes";
const PIN: &str = "123456";

struct Offline;

#[async_trait]
impl Transport for Offline {
    async fn send_direct(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn send_mailbox(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn offline_core() -> Arc<Core> {
    let store = Store::open_in_memory().await.expect("store");
    let core = Core::open(store, [4; 32], Arc::new(Offline))
        .await
        .expect("opens");
    let dir: PathBuf =
        std::env::temp_dir().join(format!("ft-game-player-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    core.set_plugins_dir(dir);
    core
}

async fn install(core: &Core, id: &str, kind: &str) {
    let catalogue = Ed25519SecretKey::new();
    let manifest = format!(
        r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"{GAMES_SINCE}","components":["ft-x"],"kind":"{kind}"}}"#
    );
    let package = sign_package(
        &[
            ("module.json".to_owned(), manifest.into_bytes()),
            ("dist/index.js".to_owned(), Vec::new()),
        ],
        &catalogue,
    );
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default())
        .await
        .expect("installs");
}

async fn games_and_a_tool() -> Arc<Core> {
    let core = offline_core().await;
    install(&core, CHESS, "game").await;
    install(&core, DRAUGHTS, "game").await;
    install(&core, NOTES, "tool").await;
    core
}

async fn someone_in(core: &Core, session: &str) {
    let other = offline_core().await;
    let link = other.my_card().await.expect("card").to_link();
    core.add_contact_in(&link, None, Some(session))
        .await
        .expect("adds inside");
}

fn player(who: &str, name: &str) -> Player {
    Player {
        who: who.to_owned(),
        hair: "curly-puff".to_owned(),
        hair_colour: "copper".to_owned(),
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn every_game_on_the_phone_sees_the_same_player() {
    let core = games_and_a_tool().await;
    assert_eq!(core.game_player(CHESS, None).await.unwrap(), None);
    assert_eq!(core.game_player(DRAUGHTS, None).await.unwrap(), None);

    core.set_game_player(CHESS, None, &player("juno", "Ana"))
        .await
        .expect("keeps");
    assert_eq!(
        core.game_player(DRAUGHTS, None).await.unwrap(),
        Some(player("juno", "Ana"))
    );

    let changed = Player {
        hair: "none".to_owned(),
        hair_colour: "ink".to_owned(),
        ..player("nico", "")
    };
    core.set_game_player(DRAUGHTS, None, &changed)
        .await
        .expect("keeps");
    assert_eq!(core.game_player(CHESS, None).await.unwrap(), Some(changed));
}

#[tokio::test]
async fn a_tool_neither_reads_nor_writes_the_player() {
    let core = games_and_a_tool().await;
    core.set_game_player(CHESS, None, &player("juno", "Ana"))
        .await
        .expect("keeps");

    assert!(core.game_player(NOTES, None).await.is_err());
    assert!(core
        .set_game_player(NOTES, None, &player("milo", "Leo"))
        .await
        .is_err());
    assert_eq!(
        core.game_player(CHESS, None).await.unwrap(),
        Some(player("juno", "Ana"))
    );
}

#[tokio::test]
async fn a_plugin_that_is_not_installed_neither_reads_nor_writes_the_player() {
    let core = games_and_a_tool().await;
    core.set_game_player(CHESS, None, &player("juno", "Ana"))
        .await
        .expect("keeps");
    let stranger = "com.example.game.go";

    assert!(core.game_player(stranger, None).await.is_err());
    assert!(core
        .set_game_player(stranger, None, &player("milo", "Leo"))
        .await
        .is_err());
    core.remove_plugin(DRAUGHTS).await.expect("removes");
    assert!(core.game_player(DRAUGHTS, None).await.is_err());
    assert_eq!(
        core.game_player(CHESS, None).await.unwrap(),
        Some(player("juno", "Ana"))
    );
}

#[tokio::test]
async fn each_session_keeps_its_own_player() {
    let core = games_and_a_tool().await;
    let session = core.open_session(PIN).await.unwrap().expect("room");
    let other = core.open_session("654321").await.unwrap().expect("room");

    core.set_game_player(CHESS, None, &player("juno", "Ana"))
        .await
        .expect("keeps");
    core.set_game_player(CHESS, Some(&session), &player("milo", "Leo"))
        .await
        .expect("keeps");

    assert_eq!(
        core.game_player(DRAUGHTS, None).await.unwrap(),
        Some(player("juno", "Ana"))
    );
    assert_eq!(
        core.game_player(DRAUGHTS, Some(&session)).await.unwrap(),
        Some(player("milo", "Leo"))
    );
    assert_eq!(
        core.game_player(DRAUGHTS, Some(&other)).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_closed_session_lends_its_player_to_nobody_and_has_it_back_when_opened() {
    let core = games_and_a_tool().await;
    let session = core.open_session(PIN).await.unwrap().expect("room");
    someone_in(&core, &session).await;
    core.set_game_player(CHESS, Some(&session), &player("milo", "Leo"))
        .await
        .expect("keeps");

    core.close_session(&session).await.expect("closes");
    assert!(core.game_player(CHESS, Some(&session)).await.is_err());
    assert!(core
        .set_game_player(CHESS, Some(&session), &player("juno", "Ana"))
        .await
        .is_err());
    assert_eq!(core.game_player(CHESS, None).await.unwrap(), None);

    assert_eq!(
        core.open_session(PIN).await.unwrap().as_deref(),
        Some(session.as_str())
    );
    assert_eq!(
        core.game_player(DRAUGHTS, Some(&session)).await.unwrap(),
        Some(player("milo", "Leo"))
    );
}

#[tokio::test]
async fn a_removed_session_takes_its_player_with_it() {
    let core = games_and_a_tool().await;
    let session = core.open_session(PIN).await.unwrap().expect("room");
    someone_in(&core, &session).await;
    core.set_game_player(CHESS, None, &player("juno", "Ana"))
        .await
        .expect("keeps");
    core.set_game_player(CHESS, Some(&session), &player("milo", "Leo"))
        .await
        .expect("keeps");

    core.remove_session(&session).await.expect("removes");
    assert_eq!(
        core.store().game_player(Some(&session)).await.unwrap(),
        None
    );
    let again = core.open_session(PIN).await.unwrap().expect("room");
    assert_eq!(core.game_player(CHESS, Some(&again)).await.unwrap(), None);
    assert_eq!(
        core.game_player(CHESS, None).await.unwrap(),
        Some(player("juno", "Ana"))
    );
}

#[tokio::test]
async fn only_a_look_from_the_cast_is_kept() {
    let core = games_and_a_tool().await;
    let looks = [
        ("flick", "none", "ink"),
        ("bob", "none", "ink"),
        ("Juno", "none", "ink"),
        ("", "none", "ink"),
        ("juno", "mohawk", "ink"),
        ("juno", "ink", "ink"),
        ("juno", "none", "blue"),
        ("juno", "none", "none"),
    ];
    for (who, hair, hair_colour) in looks {
        let wrong = Player {
            who: who.to_owned(),
            hair: hair.to_owned(),
            hair_colour: hair_colour.to_owned(),
            name: "Ana".to_owned(),
        };
        assert!(
            core.set_game_player(CHESS, None, &wrong).await.is_err(),
            "{who:?} {hair:?} {hair_colour:?}"
        );
    }
    assert_eq!(core.game_player(CHESS, None).await.unwrap(), None);
}

#[tokio::test]
async fn only_a_clean_name_of_up_to_forty_characters_is_kept() {
    let core = games_and_a_tool().await;
    let forty = "x".repeat(40);
    let forty_faces = "😀".repeat(40);
    for name in [
        "",
        "Ana",
        "Ana María de la Luz",
        "李雷",
        "Zoë 🚀",
        &forty,
        &forty_faces,
    ] {
        core.set_game_player(CHESS, None, &player("juno", name))
            .await
            .unwrap_or_else(|_| panic!("{name:?} is clean"));
        assert_eq!(
            core.game_player(CHESS, None).await.unwrap(),
            Some(player("juno", name))
        );
    }

    let kept = player("juno", "Ana");
    core.set_game_player(CHESS, None, &kept)
        .await
        .expect("keeps");
    let too_long = "x".repeat(41);
    let too_many_faces = "😀".repeat(41);
    let dirty = [
        too_long.as_str(),
        &too_many_faces,
        " Ana",
        "Ana ",
        " ",
        "An  a",
        "An\na",
        "An\ta",
        "An\u{a0}a",
        "An\u{0}a",
        "Ana\u{200e}",
        "\u{202e}Ana",
        "An\u{2066}a",
        "\u{feff}Ana",
    ];
    for name in dirty {
        assert!(
            core.set_game_player(CHESS, None, &player("juno", name))
                .await
                .is_err(),
            "{name:?} is not clean"
        );
    }
    assert_eq!(core.game_player(CHESS, None).await.unwrap(), Some(kept));
}

#[tokio::test]
async fn a_refusal_never_says_the_name_or_the_look() {
    let core = games_and_a_tool().await;
    let dirty = player("juno", "Secretname\u{202e}");
    let error = core
        .set_game_player(CHESS, None, &dirty)
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("Secretname"), "{error}");

    let outsider = Player {
        who: "stranger".to_owned(),
        ..player("juno", "Secretname")
    };
    let error = core
        .set_game_player(CHESS, None, &outsider)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        !error.contains("Secretname") && !error.contains("stranger"),
        "{error}"
    );

    let error = core
        .set_game_player(NOTES, None, &player("juno", "Secretname"))
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("Secretname"), "{error}");
}
