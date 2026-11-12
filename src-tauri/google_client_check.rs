pub const WATCHED: [&str; 6] = [
    "FT_GOOGLE_CLIENT_ID",
    "FT_GOOGLE_IOS_CLIENT_ID",
    ALLOW,
    "TAURI_FT_GOOGLE_CLIENT_ID",
    "TAURI_FT_GOOGLE_IOS_CLIENT_ID",
    "TAURI_FT_ALLOW_NO_GOOGLE_CLIENT",
];

const ALLOW: &str = "FT_ALLOW_NO_GOOGLE_CLIENT";

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    Fine,
    Warn(String),
    Fail(String),
}

pub fn google_client_check(target_os: &str, profile: &str, var: &dyn Fn(&str) -> Option<String>) -> Check {
    let name = match target_os {
        "android" => "FT_GOOGLE_CLIENT_ID",
        "ios" => "FT_GOOGLE_IOS_CLIENT_ID",
        _ => return Check::Fine,
    };
    // tauri ios build only passes TAURI*, WRY*, CARGO_*, RUST_*, TMPDIR and PATH on to the Xcode
    // build, so also accept each name with a TAURI_ prefix (the plain name wins)
    let var = |name: &str| var(name).filter(|value| !value.trim().is_empty()).or_else(|| var(&format!("TAURI_{name}")));
    let value = var(name).unwrap_or_default();
    let id = value.trim();
    let problem = if id.is_empty() {
        "is not set"
    } else if id.strip_suffix(".apps.googleusercontent.com").is_none_or(|prefix| prefix.is_empty() || prefix.contains(['/', ':'])) {
        "is not a Google OAuth client id (<id>.apps.googleusercontent.com)"
    } else {
        return Check::Fine;
    };
    let unpublished = var(ALLOW).is_some_and(|allow| allow.trim() == "1");
    let through = if target_os == "ios" {
        format!(". `tauri ios build` passes only TAURI_* variables to the Xcode build: export it as TAURI_{name} too")
    } else {
        String::new()
    };
    if profile == "release" && !unpublished {
        Check::Fail(format!(
            "{name} {problem}: this {target_os} release would ship without Google Drive sign-in. \
             Set it to the {target_os} OAuth client of the app (infra/.env, or the `release` \
             environment's secret in CI){through}; only for a release that is never published, \
             set {ALLOW}=1."
        ))
    } else {
        Check::Warn(format!("{name} {problem}: this {target_os} build cannot sign in to Google Drive."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
    }

    const ANDROID: &str = "111-android.apps.googleusercontent.com";
    const IOS: &str = "222-ios.apps.googleusercontent.com";

    fn fails(check: Check, names: &str) -> bool {
        matches!(check, Check::Fail(message) if message.contains(names))
    }

    fn warns(check: Check, names: &str) -> bool {
        matches!(check, Check::Warn(message) if message.contains(names))
    }

    #[test]
    fn a_release_for_a_phone_does_not_build_without_its_client() {
        assert!(fails(google_client_check("android", "release", &env(&[])), "FT_GOOGLE_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[("FT_GOOGLE_CLIENT_ID", ANDROID)])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert!(fails(google_client_check("android", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", IOS)])), "FT_GOOGLE_CLIENT_ID"));
        assert!(fails(google_client_check("android", "release", &env(&[("FT_GOOGLE_CLIENT_ID", " ")])), "FT_GOOGLE_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", "a-client-secret-by-mistake")])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", ".apps.googleusercontent.com")])), "FT_GOOGLE_IOS_CLIENT_ID"));
    }

    #[test]
    fn a_release_with_its_client_builds() {
        assert_eq!(google_client_check("android", "release", &env(&[("FT_GOOGLE_CLIENT_ID", ANDROID)])), Check::Fine);
        assert_eq!(google_client_check("ios", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", IOS)])), Check::Fine);
        assert_eq!(google_client_check("ios", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", "222-ios.apps.googleusercontent.com\n")])), Check::Fine);
    }

    #[test]
    fn a_debug_build_or_an_unpublished_release_only_warns() {
        assert!(warns(google_client_check("android", "debug", &env(&[])), "FT_GOOGLE_CLIENT_ID"));
        assert!(warns(google_client_check("ios", "debug", &env(&[])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert_eq!(google_client_check("ios", "debug", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", IOS)])), Check::Fine);
        assert!(warns(google_client_check("android", "release", &env(&[("FT_ALLOW_NO_GOOGLE_CLIENT", "1")])), "FT_GOOGLE_CLIENT_ID"));
        assert!(fails(google_client_check("android", "release", &env(&[("FT_ALLOW_NO_GOOGLE_CLIENT", "")])), "FT_GOOGLE_CLIENT_ID"));
        assert!(fails(google_client_check("android", "release", &env(&[("FT_ALLOW_NO_GOOGLE_CLIENT", "0")])), "FT_GOOGLE_CLIENT_ID"));
    }

    #[test]
    fn the_desktop_has_no_login_and_nothing_to_check() {
        for os in ["macos", "linux", "windows"] {
            assert_eq!(google_client_check(os, "release", &env(&[])), Check::Fine);
        }
    }

    #[test]
    fn the_variables_also_count_with_the_prefix_tauri_lets_through() {
        assert_eq!(google_client_check("ios", "release", &env(&[("TAURI_FT_GOOGLE_IOS_CLIENT_ID", IOS)])), Check::Fine);
        assert_eq!(google_client_check("android", "release", &env(&[("TAURI_FT_GOOGLE_CLIENT_ID", ANDROID)])), Check::Fine);
        assert!(warns(google_client_check("ios", "release", &env(&[("TAURI_FT_ALLOW_NO_GOOGLE_CLIENT", "1")])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[("FT_GOOGLE_IOS_CLIENT_ID", "wrong"), ("TAURI_FT_GOOGLE_IOS_CLIENT_ID", IOS)])), "FT_GOOGLE_IOS_CLIENT_ID"));
        assert!(fails(google_client_check("ios", "release", &env(&[])), "TAURI_FT_GOOGLE_IOS_CLIENT_ID"));
    }

    #[test]
    fn a_change_in_any_variable_runs_the_check_again() {
        assert_eq!(
            WATCHED,
            [
                "FT_GOOGLE_CLIENT_ID",
                "FT_GOOGLE_IOS_CLIENT_ID",
                "FT_ALLOW_NO_GOOGLE_CLIENT",
                "TAURI_FT_GOOGLE_CLIENT_ID",
                "TAURI_FT_GOOGLE_IOS_CLIENT_ID",
                "TAURI_FT_ALLOW_NO_GOOGLE_CLIENT"
            ]
        );
    }
}
