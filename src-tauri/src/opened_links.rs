use ft_contacts::{opened_link, LinkKind};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_ft_platform::PlatformExt;

pub const OPENED_LINK_EVENT: &str = "ft://opened-link";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedLinkView {
    kind: &'static str,
    link: String,
    valid: bool,
}

pub fn view_of(url: &str) -> Option<OpenedLinkView> {
    let opened = opened_link(url)?;
    let kind = match opened.kind {
        LinkKind::Add => "add",
        LinkKind::Move => "move",
    };
    Some(OpenedLinkView { kind, link: opened.link, valid: opened.valid })
}

/// Must stay async. The bridge answers on the main thread, which a sync command would block.
#[tauri::command]
pub async fn core_opened_link(app: AppHandle) -> Option<OpenedLinkView> {
    view_of(&app.platform().opened_link().ok()??)
}

#[tauri::command]
pub fn core_read_link(url: String) -> Option<OpenedLinkView> {
    view_of(&url)
}

/// Off the main thread, a bridge command blocks until the main thread answers.
pub fn listen(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        app.platform().listen_links(move || {
            let _ = emitter.emit(OPENED_LINK_EVENT, ());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_link_reaches_the_webview_as_its_page_and_its_link() {
        let view = view_of("https://flickertalk.com/add#garbage").expect("one of ours");
        assert_eq!(
            serde_json::to_value(&view).unwrap(),
            serde_json::json!({ "kind": "add", "link": "https://flickertalk.com/add#garbage", "valid": false })
        );
        let view = view_of("https://flickertalk.com/move#garbage").expect("one of ours");
        assert_eq!(serde_json::to_value(&view).unwrap()["kind"], "move");
        assert_eq!(view_of("https://flickertalk.com/add"), None);
        assert_eq!(view_of("https://www.flickertalk.com/add#garbage"), None);
    }

    fn versioned(path: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    const MANIFEST: &str = "gen/android/app/src/main/AndroidManifest.xml";
    const ENTITLEMENTS: &str = "gen/apple/flickertalk_iOS/flickertalk_iOS.entitlements";
    const PROJECT: &str = "gen/apple/flickertalk.xcodeproj/project.pbxproj";

    fn intent_filters(manifest: &str) -> Vec<&str> {
        manifest
            .split("<intent-filter")
            .skip(1)
            .filter_map(|rest| rest.split("</intent-filter>").next())
            .collect()
    }

    #[test]
    fn the_android_manifest_opens_the_add_and_move_links_verified() {
        let manifest = versioned(MANIFEST);
        let filter = intent_filters(&manifest)
            .into_iter()
            .find(|filter| filter.contains("android:autoVerify=\"true\""))
            .expect("a filter with autoVerify in the app's manifest");
        for line in [
            "<action android:name=\"android.intent.action.VIEW\" />",
            "<category android:name=\"android.intent.category.DEFAULT\" />",
            "<category android:name=\"android.intent.category.BROWSABLE\" />",
            "android:scheme=\"https\"",
            "android:host=\"flickertalk.com\"",
            "android:path=\"/add\"",
            "android:path=\"/move\"",
        ] {
            assert!(filter.contains(line), "the filter lacks {line}");
        }
        assert!(!filter.contains("pathPrefix") && !filter.contains("pathPattern"), "{filter}");
        assert!(!filter.contains("www."), "{filter}");
        let activity = manifest.split("<activity").nth(1).expect("an activity");
        assert!(activity.contains("android:name=\".MainActivity\"") && activity.contains("android:autoVerify=\"true\""));
    }

    #[test]
    fn no_flickertalk_scheme_is_claimed() {
        for path in [MANIFEST, "platform/android/src/main/AndroidManifest.xml", ENTITLEMENTS, "Info.ios.plist", "gen/apple/flickertalk_iOS/Info.plist"] {
            let file = versioned(path);
            assert!(!file.contains("android:scheme=\"flickertalk\""), "{path}");
            assert!(!file.contains("<string>flickertalk</string>"), "{path}");
            assert!(!file.contains("CFBundleURLSchemes"), "{path}");
        }
    }

    #[test]
    fn the_ios_entitlements_keep_the_domain_and_no_team_id() {
        let entitlements = versioned(ENTITLEMENTS);
        let domains = entitlements.split("<key>com.apple.developer.associated-domains</key>").nth(1).expect("associated domains");
        let domains = domains.split("</array>").next().expect("a list of domains");
        assert!(domains.contains("<string>applinks:flickertalk.com</string>"), "{domains}");
        assert!(!domains.contains("www."), "{domains}");
        assert!(!domains.contains('?'), "no mode suffix: {domains}");
        let project = versioned(PROJECT);
        assert!(project.contains("CODE_SIGN_ENTITLEMENTS = flickertalk_iOS/flickertalk_iOS.entitlements;"));
        let teams: Vec<&str> = project.lines().filter(|line| line.contains("DEVELOPMENT_TEAM")).collect();
        assert!(!teams.is_empty(), "the lines scripts/ios-build.sh fills in are there");
        for line in teams {
            assert_eq!(line.trim(), "DEVELOPMENT_TEAM = \"\";", "a team ID in the public repo");
        }
        for path in [ENTITLEMENTS, PROJECT, "gen/apple/project.yml"] {
            assert!(!has_team_id(&versioned(path)), "{path} carries a team ID");
        }
    }

    fn has_team_id(text: &str) -> bool {
        let id = |word: &str| word.len() == 10 && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        let after_team = text.split("DEVELOPMENT_TEAM").skip(1).any(|rest| {
            let value = rest.trim_start_matches([' ', '=', ':', '"', '\'']);
            id(value.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or(""))
        });
        let before_bundle = text
            .match_indices(".com.flickertalk")
            .any(|(at, _)| id(text[..at].rsplit(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("")));
        after_team || before_bundle
    }

    #[test]
    fn a_team_id_is_told_apart() {
        assert!(has_team_id("DEVELOPMENT_TEAM = AB12CD34EF;"));
        assert!(has_team_id("DEVELOPMENT_TEAM: AB12CD34EF"));
        assert!(has_team_id("<string>AB12CD34EF.com.flickertalk.app</string>"));
        assert!(!has_team_id("DEVELOPMENT_TEAM = \"\";"));
        assert!(!has_team_id("<string>applinks:flickertalk.com</string>"));
    }
}
