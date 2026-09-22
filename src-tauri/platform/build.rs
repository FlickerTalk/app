// No commands for the WebView: only the app's Rust calls this plugin.
const COMMANDS: &[&str] = &[];

fn main() {
  tauri_plugin::Builder::new(COMMANDS)
    .android_path("android")
    .ios_path("ios")
    .build();
}
