//! Keeps the new client layers free of the removed webview host.
//!
//! This is a plain `cargo test` so it runs on every client gate without
//! requiring Node or a pin to the old shell.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_IN_CORE_AND_MODEL: &[&str] = &[
    "gpui", "gpui-kit", "tauri", "wry", "webview2", "rodio", "windows",
];

const FORBIDDEN_IN_APP_MANIFEST: &[&str] = &["tauri", "wry", "webview2", "@tauri-apps"];

const FORBIDDEN_IN_CORE_AND_MODEL_SOURCE: &[&str] = &[
    "gpui::",
    "tauri::",
    "@tauri-apps",
    "invoke(",
    "wry::",
    "webview2",
];

const FORBIDDEN_IN_APP_SOURCE: &[&str] =
    &["tauri::", "@tauri-apps", "invoke(", "wry::", "webview2"];

fn client_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("client directory")
        .to_path_buf()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", directory.display()))
    {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn client_layers_do_not_reintroduce_tauri_or_webview() {
    let client = client_dir();

    for layer in ["core", "model"] {
        let manifest = read(&client.join(layer).join("Cargo.toml")).to_lowercase();
        for needle in FORBIDDEN_IN_CORE_AND_MODEL {
            assert!(
                !manifest.contains(needle),
                "client/{layer}/Cargo.toml must not depend on `{needle}`"
            );
        }

        for path in rust_files(&client.join(layer).join("src")) {
            let source = read(&path);
            for needle in FORBIDDEN_IN_CORE_AND_MODEL_SOURCE {
                assert!(
                    !source.contains(needle),
                    "client/{layer}/src must not contain `{needle}`: {}",
                    path.display()
                );
            }
        }
    }

    let app_manifest = read(&client.join("app").join("Cargo.toml")).to_lowercase();
    for needle in FORBIDDEN_IN_APP_MANIFEST {
        assert!(
            !app_manifest.contains(needle),
            "client/app/Cargo.toml must not depend on `{needle}`"
        );
    }

    for path in rust_files(&client.join("app").join("src")) {
        let source = read(&path);
        for needle in FORBIDDEN_IN_APP_SOURCE {
            assert!(
                !source.contains(needle),
                "client/app/src must not contain `{needle}`: {}",
                path.display()
            );
        }
    }
}
