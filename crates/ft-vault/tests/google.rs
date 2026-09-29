//! The Google Drive adapter against a fake Drive (plan-drive §6): the folder, files by name,
//! the resumable upload in parts, downloads, the quota, and the tokens renewing themselves.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use ft_vault::google::{exchange, GoogleDrive, MemoryTokens, TokenKeeper, Tokens};
use ft_vault::{quiet, Provider, Quota, Vault};
use serde_json::{json, Value};

#[derive(Clone)]
struct Stored {
    name: String,
    parents: Vec<String>,
    mime: String,
    bytes: Vec<u8>,
}

/// A resumable upload in progress: the file's name, its folder and the bytes so far.
type Session = (String, Option<String>, Vec<u8>);

#[derive(Default)]
struct Drive {
    files: Mutex<HashMap<String, Stored>>,
    sessions: Mutex<HashMap<String, Session>>,
    next: Mutex<u64>,
    tokens_given: Mutex<u32>,
    puts: Mutex<u32>,
}

impl Drive {
    fn id(&self) -> String {
        let mut next = self.next.lock().unwrap();
        *next += 1;
        format!("id{}", *next)
    }
}

fn authorized(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|value| value.to_str().ok()).is_some_and(|value| value.starts_with("Bearer good-"))
}

/// Google's query language, the little of it the adapter uses.
fn matches(query: &str, id: &str, file: &Stored) -> bool {
    for clause in query.split(" and ") {
        let clause = clause.trim();
        if let Some(rest) = clause.strip_prefix("name = '") {
            if rest.trim_end_matches('\'') != file.name {
                return false;
            }
        } else if let Some(rest) = clause.strip_prefix("mimeType = '") {
            if rest.trim_end_matches('\'') != file.mime {
                return false;
            }
        } else if let Some(rest) = clause.strip_prefix('\'') {
            let parent = rest.split('\'').next().unwrap_or_default();
            if !file.parents.iter().any(|one| one == parent) {
                return false;
            }
        } else if clause == "trashed = false" {
            continue;
        } else {
            panic!("the fake Drive does not know {clause:?} ({id})");
        }
    }
    true
}

async fn fake_drive() -> (String, Arc<Drive>) {
    let drive = Arc::new(Drive::default());
    let router = Router::new()
        .route(
            "/token",
            post(|State(drive): State<Arc<Drive>>, body: String| async move {
                let mut given = drive.tokens_given.lock().unwrap();
                *given += 1;
                if body.contains("grant_type=authorization_code") {
                    assert!(body.contains("code_verifier="), "PKCE");
                    assert!(body.contains("code=the-code"));
                    Json(json!({ "access_token": format!("good-{}", *given), "refresh_token": "refresh-1", "expires_in": 3600 }))
                } else if body.contains("grant_type=refresh_token") && body.contains("refresh_token=refresh-1") {
                    Json(json!({ "access_token": format!("good-{}", *given), "expires_in": 3600 }))
                } else {
                    Json(json!({ "error": "invalid_grant" }))
                }
            }),
        )
        .route(
            "/drive/v3/files",
            get(|State(drive): State<Arc<Drive>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let q = query.get("q").cloned().unwrap_or_default();
                let files = drive.files.lock().unwrap();
                let found: Vec<Value> = files
                    .iter()
                    .filter(|(id, file)| matches(&q, id, file))
                    .map(|(id, file)| json!({ "id": id, "name": file.name, "size": file.bytes.len().to_string() }))
                    .collect();
                Json(json!({ "files": found })).into_response()
            })
            .post(|State(drive): State<Arc<Drive>>, headers: HeaderMap, Json(body): Json<Value>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let id = drive.id();
                drive.files.lock().unwrap().insert(
                    id.clone(),
                    Stored {
                        name: body["name"].as_str().unwrap().to_owned(),
                        parents: vec![],
                        mime: body["mimeType"].as_str().unwrap_or("application/octet-stream").to_owned(),
                        bytes: vec![],
                    },
                );
                Json(json!({ "id": id, "name": body["name"] })).into_response()
            }),
        )
        .route(
            "/drive/v3/files/{id}",
            get(|State(drive): State<Arc<Drive>>, Path(id): Path<String>, headers: HeaderMap| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                match drive.files.lock().unwrap().get(&id) {
                    Some(file) => file.bytes.clone().into_response(),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            })
            .delete(|State(drive): State<Arc<Drive>>, Path(id): Path<String>| async move {
                drive.files.lock().unwrap().remove(&id);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/drive/v3/about",
            get(|| async { Json(json!({ "storageQuota": { "usage": "123", "limit": "1000" } })) }),
        )
        .route(
            "/upload/drive/v3/files",
            post(|State(drive): State<Arc<Drive>>, headers: HeaderMap, Json(body): Json<Value>| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let session = drive.id();
                let parent = body["parents"][0].as_str().map(str::to_owned);
                drive.sessions.lock().unwrap().insert(session.clone(), (body["name"].as_str().unwrap().to_owned(), parent, vec![]));
                let mut answer = HeaderMap::new();
                answer.insert("location", format!("/upload/session/{session}").parse().unwrap());
                (StatusCode::OK, answer).into_response()
            }),
        )
        .route(
            "/upload/drive/v3/files/{id}",
            axum::routing::patch(|State(drive): State<Arc<Drive>>, Path(id): Path<String>, headers: HeaderMap| async move {
                if !authorized(&headers) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let session = drive.id();
                let existing = drive.files.lock().unwrap().get(&id).cloned().expect("updates an existing file");
                drive.sessions.lock().unwrap().insert(session.clone(), (format!("replace:{id}"), existing.parents.first().cloned(), vec![]));
                let mut answer = HeaderMap::new();
                answer.insert("location", format!("/upload/session/{session}").parse().unwrap());
                (StatusCode::OK, answer).into_response()
            }),
        )
        .route(
            "/upload/session/{session}",
            axum::routing::put(|State(drive): State<Arc<Drive>>, Path(session): Path<String>, headers: HeaderMap, body: Bytes| async move {
                *drive.puts.lock().unwrap() += 1;
                let range = headers.get("content-range").unwrap().to_str().unwrap().to_owned();
                let (name, parent, mut bytes) = drive.sessions.lock().unwrap().remove(&session).expect("a session");
                bytes.extend_from_slice(&body);
                let (spec, total) = range.strip_prefix("bytes ").unwrap().split_once('/').unwrap();
                let total: usize = total.parse().unwrap();
                if spec != "*" {
                    let (_, end) = spec.split_once('-').unwrap();
                    let end: usize = end.parse().unwrap();
                    assert_eq!(bytes.len(), end + 1, "parts arrive in order");
                }
                if bytes.len() < total {
                    drive.sessions.lock().unwrap().insert(session, (name, parent, bytes.clone()));
                    let mut answer = HeaderMap::new();
                    answer.insert("range", format!("bytes=0-{}", bytes.len() - 1).parse().unwrap());
                    return (StatusCode::PERMANENT_REDIRECT, answer).into_response();
                }
                let mut files = drive.files.lock().unwrap();
                let id = match name.strip_prefix("replace:") {
                    Some(id) => {
                        files.get_mut(id).unwrap().bytes = bytes;
                        id.to_owned()
                    }
                    None => {
                        let id = drive.id();
                        files.insert(id.clone(), Stored { name: name.clone(), parents: parent.into_iter().collect(), mime: "application/octet-stream".into(), bytes });
                        id
                    }
                };
                Json(json!({ "id": id, "name": name })).into_response()
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(drive.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{address}"), drive)
}

fn response_ok(response: Response) -> bool {
    response.status().is_success()
}

#[tokio::test]
async fn the_login_gives_tokens_and_the_drive_keeps_its_files_in_the_apps_folder() {
    let (base, drive) = fake_drive().await;
    let http = ft_push::https_client(std::time::Duration::from_secs(10)).unwrap();
    let tokens = exchange(&http, &format!("{base}/token"), "client", "com.flickertalk.app:/oauth", "the-code", "verifier").await.unwrap();
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
    assert!(tokens.expires_at > 0);
    let keeper = Arc::new(MemoryTokens::default());
    keeper.keep(&tokens).await.unwrap();

    let google = GoogleDrive::at(&base, &format!("{base}/token"), "client", keeper.clone()).unwrap();
    assert_eq!(google.read("vault.json").await.unwrap(), None);
    google.write("vault.json", b"{}".to_vec()).await.unwrap();
    assert_eq!(google.read("vault.json").await.unwrap(), Some(b"{}".to_vec()));
    google.write("vault.json", b"{\"v\":2}".to_vec()).await.unwrap();
    assert_eq!(google.read("vault.json").await.unwrap(), Some(b"{\"v\":2}".to_vec()), "replaced, not duplicated");
    {
        let files = drive.files.lock().unwrap();
        let folder = files.values().find(|file| file.mime == "application/vnd.google-apps.folder").expect("the app's folder");
        assert_eq!(folder.name, "FlickerTalk");
        assert_eq!(files.values().filter(|file| file.name == "vault.json").count(), 1);
        let (folder_id, _) = files.iter().find(|(_, file)| file.mime == "application/vnd.google-apps.folder").unwrap();
        assert!(files.values().filter(|file| file.name == "vault.json").all(|file| file.parents == [folder_id.clone()]), "inside the folder");
    }

    // A big file goes up in parts and comes down whole.
    let home = std::env::temp_dir().join(format!("ft-vault-google-{}", ft_vault::index::new_id()));
    std::fs::create_dir_all(&home).unwrap();
    let big: Vec<u8> = (0..20_000_000u32).map(|at| (at % 251) as u8).collect();
    std::fs::write(home.join("big"), &big).unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let progress = {
        let seen = seen.clone();
        Arc::new(move |done: u64, total: u64| seen.lock().unwrap().push((done, total)))
    };
    google.upload("blob-1", &home.join("big"), progress).await.unwrap();
    assert!(*drive.puts.lock().unwrap() >= 3, "8 MiB parts");
    assert_eq!(seen.lock().unwrap().last().copied(), Some((big.len() as u64, big.len() as u64)));
    google.download("blob-1", &home.join("down"), quiet()).await.unwrap();
    assert_eq!(std::fs::read(home.join("down")).unwrap(), big);
    let mut listed = google.list().await.unwrap();
    listed.sort();
    assert_eq!(listed, [("blob-1".to_owned(), big.len() as u64), ("vault.json".to_owned(), 7)]);
    assert_eq!(google.quota().await.unwrap(), Some(Quota { used: 123, total: 1000 }));
    google.remove("blob-1").await.unwrap();
    google.remove("blob-1").await.unwrap();
    assert_eq!(google.list().await.unwrap().len(), 1);
    assert!(google.download("blob-1", &home.join("gone"), quiet()).await.is_err());

    // An expired access token is renewed with the refresh token, and the new one is kept.
    let given = *drive.tokens_given.lock().unwrap();
    keeper.keep(&Tokens { access_token: "good-old".into(), refresh_token: Some("refresh-1".into()), expires_at: 1 }).await.unwrap();
    assert_eq!(google.read("vault.json").await.unwrap(), Some(b"{\"v\":2}".to_vec()));
    assert_eq!(*drive.tokens_given.lock().unwrap(), given + 1);
    assert!(keeper.tokens().await.unwrap().expires_at > 1);
    // Without a refresh token, or logged out, it says so.
    keeper.keep(&Tokens { access_token: "good-x".into(), refresh_token: None, expires_at: 1 }).await.unwrap();
    assert!(google.read("vault.json").await.is_err());
    let nobody = GoogleDrive::at(&base, &format!("{base}/token"), "client", Arc::new(MemoryTokens::default())).unwrap();
    assert!(nobody.read("vault.json").await.is_err());
    let _ = response_ok;
}

// Found on a real Drive (2026-09-27): two calls at once each looked for the app's folder, found
// none and made one, and a later start could pick the empty one. There is one folder, always.
#[tokio::test]
async fn calls_at_once_make_one_folder() {
    let (base, drive) = fake_drive().await;
    let keeper = Arc::new(MemoryTokens::default());
    keeper.keep(&Tokens { access_token: "good-1".into(), refresh_token: Some("refresh-1".into()), expires_at: i64::MAX }).await.unwrap();
    let google = GoogleDrive::at(&base, &format!("{base}/token"), "client", keeper).unwrap();
    let (a, b, c) = tokio::join!(google.read("vault.json"), google.read("key.ftv"), google.list());
    a.unwrap();
    b.unwrap();
    c.unwrap();
    let folders = drive.files.lock().unwrap().values().filter(|file| file.mime == "application/vnd.google-apps.folder").count();
    assert_eq!(folders, 1);
}

#[tokio::test]
async fn a_whole_drive_lives_on_google() {
    let (base, _drive) = fake_drive().await;
    let keeper = Arc::new(MemoryTokens::default());
    keeper.keep(&Tokens { access_token: "good-1".into(), refresh_token: None, expires_at: i64::MAX }).await.unwrap();
    let google: Arc<dyn Provider> = Arc::new(GoogleDrive::at(&base, &format!("{base}/token"), "client", keeper.clone()).unwrap());
    let home = std::env::temp_dir().join(format!("ft-vault-google-{}", ft_vault::index::new_id()));
    std::fs::create_dir_all(&home).unwrap();
    let vault = Vault::create(google.clone(), home.join("vault"), "phone-a", "a long phrase of mine").await.unwrap();
    std::fs::write(home.join("a.txt"), b"hello drive").unwrap();
    let id = vault.upload(&home.join("a.txt"), "a.txt", "text/plain", None, quiet()).await.unwrap().unwrap();
    let again: Arc<dyn Provider> = Arc::new(GoogleDrive::at(&base, &format!("{base}/token"), "client", keeper).unwrap());
    let recovered = Vault::recover(again, home.join("vault2"), "phone-b", "a long phrase of mine").await.unwrap();
    recovered.download(&id, &home.join("b.txt"), quiet()).await.unwrap();
    assert_eq!(std::fs::read(home.join("b.txt")).unwrap(), b"hello drive");
}
