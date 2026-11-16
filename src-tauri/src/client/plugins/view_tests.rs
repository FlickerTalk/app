use super::*;

fn entry(id: &str, version: &str) -> ft_plugins::CatalogueEntry {
    ft_plugins::CatalogueEntry {
        id: id.to_owned(),
        name: "Tool".to_owned(),
        version: version.to_owned(),
        min_core_version: "0.1.0".to_owned(),
        size: 10,
        hash: "ab".repeat(32),
        url: format!("{}/{id}/{version}.ftplugin", ft_core::CATALOGUE_HOME),
        summary: "Does a thing.".to_owned(),
        kind: ft_plugins::Kind::Tool,
        locales: Default::default(),
        icon: String::new(),
        image: String::new(),
    }
}

fn carried(id: &str, version: &str) -> OfferedPlugin {
    OfferedPlugin {
        id: id.to_owned(),
        name: "Tool".to_owned(),
        version: version.to_owned(),
        summary: "Does a thing.".to_owned(),
        size: 4096,
        installed: false,
        carried: true,
        kind: ft_plugins::Kind::Tool,
        locales: Default::default(),
        icon: String::new(),
        image: String::new(),
    }
}

#[test]
fn the_list_joins_what_is_carried_and_what_is_offered_keeping_the_newer() {
    let seeds = vec![
        carried("com.flickertalk.sketch", "1.0.0"),
        carried("com.flickertalk.pdf", "2.0.0"),
    ];
    let listed = vec![
        entry("com.flickertalk.sketch", "1.1.0"),
        entry("com.flickertalk.ocr", "1.0.0"),
    ];

    let offered = merged(seeds, &listed, &[]);
    let by_id = |id: &str| {
        offered
            .iter()
            .find(|one| one.id == id)
            .expect("listed")
            .clone()
    };

    assert_eq!(
        by_id("com.flickertalk.sketch").version,
        "1.1.0",
        "the catalogue has a newer one"
    );
    assert!(
        !by_id("com.flickertalk.sketch").carried,
        "so it is a download"
    );
    assert_eq!(
        by_id("com.flickertalk.pdf").version,
        "2.0.0",
        "nothing newer is offered"
    );
    assert!(by_id("com.flickertalk.pdf").carried);
    assert_eq!(
        by_id("com.flickertalk.ocr").size,
        10,
        "what is only in the catalogue is a download"
    );
    assert!(!by_id("com.flickertalk.ocr").carried);
    assert_eq!(offered.len(), 3);
}

#[test]
fn the_list_keeps_whether_each_one_is_a_tool_or_a_game() {
    let seeds = vec![carried("com.flickertalk.sketch", "1.0.0")];
    let chess = ft_plugins::CatalogueEntry {
        kind: ft_plugins::Kind::Game,
        ..entry("com.flickertalk.game.chess", "1.0.0")
    };
    let offered = serde_json::to_value(merged(seeds, &[chess], &[])).unwrap();
    let kinds: Vec<(&str, &str)> = offered
        .as_array()
        .unwrap()
        .iter()
        .map(|one| {
            (
                one["id"].as_str().unwrap(),
                one["kind"].as_str().unwrap_or("missing"),
            )
        })
        .collect();
    assert!(
        kinds.contains(&("com.flickertalk.game.chess", "game")),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("com.flickertalk.sketch", "tool")),
        "{kinds:?}"
    );
}

#[test]
fn an_installed_plugin_says_whether_it_is_a_game() {
    let manifest: ft_plugins::Manifest = serde_json::from_str(
        r#"{"id":"com.flickertalk.game.chess","name":"Chess","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-chess"],"kind":"game","permissions":{"live":true}}"#,
    )
    .unwrap();
    let view = plugin_view(ft_core::plugins::InstalledPlugin {
        manifest,
        granted: Default::default(),
        installed_at: 7,
        image: String::new(),
    });
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["kind"], "game");
    assert_eq!(
        (json["id"].as_str(), json["installedAt"].as_i64()),
        (Some("com.flickertalk.game.chess"), Some(7))
    );
    assert_eq!(
        (
            json["asks"]["live"].as_bool(),
            json["granted"]["live"].as_bool()
        ),
        (Some(true), Some(false))
    );
}

#[test]
fn every_view_of_a_plugin_keeps_its_translations() {
    let manifest: ft_plugins::Manifest = serde_json::from_str(
        r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"summary":"A list.","locales":{"es":{"name":"Listas","summary":"Una lista."}}}"#,
    )
    .unwrap();
    let view = plugin_view(ft_core::plugins::InstalledPlugin {
        manifest: manifest.clone(),
        granted: Default::default(),
        installed_at: 7,
        image: String::new(),
    });
    assert_eq!(
        serde_json::to_value(&view).unwrap()["locales"]["es"]["name"],
        "Listas"
    );

    let seed = serde_json::to_value(offered_from(manifest, String::new(), 4096)).unwrap();
    assert_eq!(
        (
            seed["locales"]["es"]["summary"].as_str(),
            seed["carried"].as_bool()
        ),
        (Some("Una lista."), Some(true))
    );

    let spanish = |name: &str| {
        [(
            "es".to_owned(),
            ft_plugins::Localized {
                name: Some(name.to_owned()),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect()
    };
    let seeds = vec![
        OfferedPlugin {
            locales: spanish("Dibujo"),
            ..carried("com.flickertalk.sketch", "1.0.0")
        },
        OfferedPlugin {
            locales: spanish("Imagen"),
            ..carried("com.flickertalk.images", "2.0.0")
        },
    ];
    let listed = vec![
        ft_plugins::CatalogueEntry {
            locales: spanish("Boceto"),
            ..entry("com.flickertalk.sketch", "1.1.0")
        },
        ft_plugins::CatalogueEntry {
            locales: spanish("Foto"),
            ..entry("com.flickertalk.images", "1.0.0")
        },
        ft_plugins::CatalogueEntry {
            locales: spanish("Ajedrez"),
            ..entry("com.flickertalk.game.chess", "1.0.0")
        },
    ];
    let offered = serde_json::to_value(merged(seeds, &listed, &[])).unwrap();
    let spanish_of = |id: &str| {
        let one = offered
            .as_array()
            .unwrap()
            .iter()
            .find(|one| one["id"] == id)
            .unwrap();
        one["locales"]["es"]["name"]
            .as_str()
            .unwrap_or("missing")
            .to_owned()
    };
    assert_eq!(
        spanish_of("com.flickertalk.sketch"),
        "Boceto",
        "the catalogue's is newer"
    );
    assert_eq!(
        spanish_of("com.flickertalk.images"),
        "Imagen",
        "the app's is newer"
    );
    assert_eq!(
        spanish_of("com.flickertalk.game.chess"),
        "Ajedrez",
        "only in the catalogue"
    );
}

#[test]
fn an_installed_tool_says_which_hooks_it_binds_to() {
    let manifest: ft_plugins::Manifest = serde_json::from_str(
        r#"{"id":"com.flickertalk.translate","name":"Translator","version":"1.1.0","minCoreVersion":"1.7.0","components":["ft-translate"],"permissions":{"messages":"given","send":"propose"},"hooks":{"message":{"label":"Translate","icon":"language-outline"},"settings":{"label":"Translation","icon":"language-outline","fields":[{"key":"lang","type":"select","label":"Language"},{"key":"receive","type":"toggle","label":"Translate what I receive"}],"note":"Packs are downloaded once."}},"locales":{"es":{"name":"Traductor","hooks":{"message":"Traducir","settings":{"label":"Traducción","fields":{"lang":"Idioma"}}}}}}"#,
    )
    .unwrap();
    let plain = ft_plugins::Manifest {
        hooks: Default::default(),
        ..manifest.clone()
    };
    let view = |manifest: ft_plugins::Manifest| {
        serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin {
            manifest,
            granted: Default::default(),
            installed_at: 7,
            image: String::new(),
        }))
        .unwrap()
    };
    let json = view(manifest);
    assert_eq!(
        json["hooks"],
        serde_json::json!({
            "message": { "label": "Translate", "icon": "language-outline" },
            "settings": {
                "label": "Translation",
                "icon": "language-outline",
                "fields": [
                    { "key": "lang", "type": "select", "label": "Language" },
                    { "key": "receive", "type": "toggle", "label": "Translate what I receive" }
                ],
                "note": "Packs are downloaded once."
            }
        })
    );
    assert_eq!(
        json["locales"]["es"]["hooks"]["settings"]["fields"]["lang"],
        "Idioma"
    );
    assert!(
        view(plain).get("hooks").is_none(),
        "bound to none, nothing written"
    );
}

#[test]
fn every_view_of_a_plugin_keeps_its_icon() {
    let drawn: ft_plugins::Manifest = serde_json::from_str(
        r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"icon":"list-outline"}"#,
    )
    .unwrap();
    let plain = ft_plugins::Manifest {
        icon: String::new(),
        ..drawn.clone()
    };
    let installed = |manifest: &ft_plugins::Manifest| {
        serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin {
            manifest: manifest.clone(),
            granted: Default::default(),
            installed_at: 7,
            image: String::new(),
        }))
        .unwrap()
    };
    assert_eq!(installed(&drawn)["icon"], "list-outline");
    assert!(
        installed(&plain).get("icon").is_none(),
        "no icon, nothing written"
    );

    let seed = serde_json::to_value(offered_from(drawn, String::new(), 4096)).unwrap();
    assert_eq!(seed["icon"], "list-outline");
    assert!(
        serde_json::to_value(offered_from(plain, String::new(), 4096))
            .unwrap()
            .get("icon")
            .is_none()
    );

    let listed = vec![
        ft_plugins::CatalogueEntry {
            icon: "brush-outline".to_owned(),
            ..entry("com.flickertalk.sketch", "1.1.0")
        },
        entry("com.flickertalk.pdf", "1.0.0"),
    ];
    let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
    let icon_of = |id: &str| {
        offered
            .as_array()
            .unwrap()
            .iter()
            .find(|one| one["id"] == id)
            .unwrap()
            .get("icon")
            .cloned()
    };
    assert_eq!(
        icon_of("com.flickertalk.sketch"),
        Some(serde_json::json!("brush-outline"))
    );
    assert_eq!(icon_of("com.flickertalk.pdf"), None);
}

#[test]
fn every_view_of_a_plugin_keeps_its_image() {
    const IMAGE: &str = r#"<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>"#;
    let manifest: ft_plugins::Manifest = serde_json::from_str(
        r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"]}"#,
    )
    .unwrap();
    let installed = |image: &str| {
        serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin {
            manifest: manifest.clone(),
            granted: Default::default(),
            installed_at: 7,
            image: image.to_owned(),
        }))
        .unwrap()
    };
    assert_eq!(installed(IMAGE)["image"], IMAGE);
    assert!(
        installed("").get("image").is_none(),
        "no image, nothing written"
    );
    assert_eq!(
        serde_json::to_value(offered_from(manifest.clone(), IMAGE.to_owned(), 4096)).unwrap()
            ["image"],
        IMAGE
    );
    let listed = vec![
        ft_plugins::CatalogueEntry {
            image: IMAGE.to_owned(),
            ..entry("com.flickertalk.sketch", "1.1.0")
        },
        entry("com.flickertalk.pdf", "1.0.0"),
    ];
    let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
    let image_of = |id: &str| {
        offered
            .as_array()
            .unwrap()
            .iter()
            .find(|one| one["id"] == id)
            .unwrap()
            .get("image")
            .cloned()
    };
    assert_eq!(
        image_of("com.flickertalk.sketch"),
        Some(serde_json::json!(IMAGE))
    );
    assert_eq!(image_of("com.flickertalk.pdf"), None);
}

#[test]
fn a_tool_already_here_shows_as_installed() {
    let here = ["com.flickertalk.sketch".to_owned()];
    let offered = merged(vec![carried("com.flickertalk.sketch", "1.0.0")], &[], &here);
    assert!(offered[0].installed);
}
