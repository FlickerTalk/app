use super::*;

#[test]
fn the_content_security_policy_matches_what_was_granted() {
    let none = Permissions::default();
    assert!(none
        .content_security_policy("com.example.ai")
        .contains("connect-src 'none'"));
    assert!(none
        .content_security_policy("com.example.ai")
        .starts_with("default-src 'none'"));
    let policy = none.content_security_policy("com.example.ai");
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
        .content_security_policy("com.example.ai")
        .contains("connect-src https://api.openai.com"));
    assert!(!ai
        .content_security_policy("com.example.ai")
        .contains("'unsafe-eval'"));
}

#[test]
fn the_policy_is_one_line_without_double_spaces() {
    let policy = Permissions {
        network: vec!["api.openai.com".to_owned()],
        ..Permissions::default()
    }
    .content_security_policy("com.example.ai");
    assert!(!policy.contains('\n') && !policy.contains("  "), "{policy}");
    assert!(
        policy.ends_with("frame-ancestors http://tauri.localhost tauri://localhost"),
        "{policy}"
    );
}

const SCHEME: [&str; 3] = [
    "http://ftplugin.localhost",
    "https://ftplugin.localhost",
    "ftplugin://localhost",
];

const FETCHES: [&str; 10] = [
    "script-src",
    "style-src",
    "img-src",
    "font-src",
    "connect-src",
    "media-src",
    "object-src",
    "worker-src",
    "frame-src",
    "manifest-src",
];

fn directive<'a>(policy: &'a str, name: &str) -> Option<Vec<&'a str>> {
    policy.split(';').find_map(|part| {
        let mut words = part.split_whitespace();
        (words.next() == Some(name)).then(|| words.collect())
    })
}

fn sources<'a>(policy: &'a str, name: &str) -> Vec<&'a str> {
    let order = match name {
        "worker-src" => vec!["worker-src", "child-src", "script-src", "default-src"],
        "frame-src" => vec!["frame-src", "child-src", "default-src"],
        _ => vec![name, "default-src"],
    };
    order
        .into_iter()
        .find_map(|one| directive(policy, one))
        .unwrap_or_default()
}

fn percent_decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let escaped = path
            .get(at + 1..at + 3)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[at], escaped) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn same_scheme(source: &str, url: &str) -> bool {
    source.eq_ignore_ascii_case(url) || (source == "http" && url == "https")
}

fn source_matches(source: &str, url: &str) -> bool {
    let (scheme, rest) = url.split_once("://").unwrap_or((url, ""));
    let scheme = scheme.split_once(':').map_or(scheme, |(scheme, _)| scheme);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{}", path.split(['?', '#']).next().unwrap_or(""));
    if source == "*" {
        return true;
    }
    if let Some(only) = source.strip_suffix(':') {
        return same_scheme(only, scheme);
    }
    let Some((wanted_scheme, wanted)) = source.split_once("://") else {
        return false;
    };
    let (wanted_host, wanted_path) = match wanted.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (wanted, String::new()),
    };
    let host_matches = match wanted_host.strip_prefix('*') {
        Some(suffix) => host.ends_with(suffix),
        None => wanted_host.eq_ignore_ascii_case(host),
    };
    let path = percent_decoded(&path);
    let path_matches = if wanted_path.ends_with('/') {
        path.starts_with(&wanted_path)
    } else {
        wanted_path.is_empty() || path == wanted_path
    };
    same_scheme(wanted_scheme, scheme) && host_matches && path_matches
}

fn allows(policy: &str, name: &str, url: &str) -> bool {
    sources(policy, name)
        .iter()
        .any(|source| source_matches(source, url))
}

fn directive_names(policy: &str) -> Vec<&str> {
    policy
        .split(';')
        .map(|part| part.split_whitespace().next().unwrap_or(""))
        .collect()
}

#[test]
fn a_frame_loads_its_own_files_and_the_ones_the_app_lends_it() {
    let policy = Permissions::default().content_security_policy("com.example.a");
    let own = [
        "frame.js?v=1.0.0",
        "ionic/ionic.js?v=9.0.4",
        "ionic/ionic.css?v=9.0.4",
        "ionic/theme.js?v=9.0.4",
        "icon/pencil-outline.svg",
        "dist/index.js?v=1.0.0",
        "dist/i18n.js",
        "dist/fonts/KaTeX_Main-Regular.woff2",
    ];
    for origin in SCHEME {
        for file in own {
            let url = format!("{origin}/com.example.a/{file}");
            for name in ["script-src", "style-src", "img-src", "font-src"] {
                assert!(
                    allows(&policy, name, &url),
                    "{name} refuses {url}: {policy}"
                );
            }
        }
    }
    assert!(allows(&policy, "img-src", "data:image/png;base64,AAAA"));
    assert!(allows(&policy, "img-src", "blob:ftplugin://localhost/x"));
}

#[test]
fn a_frame_cannot_load_any_file_of_another_plugin() {
    let policy = Permissions {
        network: vec!["api.example.com".to_owned()],
        ..Permissions::default()
    }
    .content_security_policy("com.example.a");
    let others = [
        "com.example.b",
        "com.example.ab",
        "com.example.a-b",
        "com.example.a.b",
        "com.example",
    ];
    let files = [
        "frame.html",
        "frame.js",
        "dist/index.js",
        "ionic/ionic.js",
        "icon/pencil-outline.svg",
        "icon.svg",
        "module.json",
    ];
    for origin in SCHEME {
        for other in others {
            for file in files {
                let url = format!("{origin}/{other}/{file}");
                for name in FETCHES {
                    assert!(
                        !allows(&policy, name, &url),
                        "{name} lets com.example.a load {url}: {policy}"
                    );
                }
            }
        }
    }
}

#[test]
fn an_id_that_begins_another_reaches_neither_way() {
    let pairs = [
        ("com.example.code", "com.example.codex"),
        ("com.example.codex", "com.example.code"),
        ("com.example.code", "com.example.code.x"),
    ];
    for (own, other) in pairs {
        let policy = Permissions::default().content_security_policy(own);
        for origin in SCHEME {
            let mine = format!("{origin}/{own}/dist/index.js");
            let theirs = format!("{origin}/{other}/dist/index.js");
            assert!(
                allows(&policy, "script-src", &mine),
                "{own} cannot load {mine}: {policy}"
            );
            assert!(
                !allows(&policy, "script-src", &theirs),
                "{own} loads {theirs}: {policy}"
            );
        }
    }
}

#[test]
fn the_policy_names_the_plugin_folder_in_every_form_of_the_scheme() {
    let policy = Permissions::default().content_security_policy("com.example.a");
    let own = "http://ftplugin.localhost/com.example.a/ https://ftplugin.localhost/com.example.a/ \
               ftplugin://localhost/com.example.a/";
    assert!(policy.contains(&format!("; script-src {own};")), "{policy}");
    assert!(
        policy.contains(&format!("; style-src {own} 'unsafe-inline';")),
        "{policy}"
    );
    assert!(
        policy.contains(&format!("; img-src {own} data: blob:;")),
        "{policy}"
    );
    assert!(policy.contains(&format!("; font-src {own};")), "{policy}");
}

#[test]
fn an_id_cannot_widen_the_policy() {
    let fair = Permissions::default().content_security_policy("com.example.a");
    let crafted = [
        "",
        "*",
        "..",
        "com.example.a * x",
        "com.example.a/ ftplugin://localhost",
        "com.example.a/ http://ftplugin.localhost/com.example.b",
        "com.example.a; script-src *",
        "com.example.a,script-src *",
        "com.example.a\nscript-src *",
        "com.example.a/../com.example.b",
        "com.example.a%2F..%2Fcom.example.b",
        "com.example.*",
        "COM.EXAMPLE.B",
        "com.example.a 'unsafe-eval'",
    ];
    for id in crafted {
        let policy = Permissions::default().content_security_policy(id);
        assert_eq!(
            directive_names(&policy),
            directive_names(&fair),
            "{id:?}: {policy}"
        );
        assert!(
            !policy.contains('\n') && !policy.contains(','),
            "{id:?}: {policy}"
        );
        assert!(!policy.contains("'unsafe-eval'"), "{id:?}: {policy}");
        for origin in SCHEME {
            let theirs = format!("{origin}/com.example.b/dist/index.js");
            for name in FETCHES {
                assert!(
                    !allows(&policy, name, &theirs),
                    "{id:?} lets {name} load {theirs}: {policy}"
                );
            }
        }
    }
}
