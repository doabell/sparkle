use std::fs;
use std::path::Path;

#[test]
fn packaged_frontend_assets_are_complete_and_current() {
    let context: tauri::Context<tauri::Wry> = tauri::tauri_build_context!();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../build");
    let mut directories = vec![root.clone()];
    let mut files = 0;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            let key = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let embedded = context
                .assets()
                .get(&key.clone().into())
                .unwrap_or_else(|| panic!("missing or corrupt embedded asset: {key}"));
            assert!(!embedded.is_empty(), "empty embedded asset: {key}");
            // Tauri injects CSP metadata into HTML; other assets retain their bytes.
            if path.extension().is_none_or(|extension| extension != "html") {
                assert!(
                    embedded.as_ref() == fs::read(&path).unwrap(),
                    "stale or incomplete embedded asset: {key}"
                );
            }
            files += 1;
        }
    }
    assert!(files > 0, "no frontend assets were checked");
    assert_eq!(
        files,
        context.assets().iter().count(),
        "obsolete embedded assets"
    );
}
