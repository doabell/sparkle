fn main() {
    tauri_build::build();
    if !tauri_build::is_dev() {
        // Also detect new/deleted assets; include_bytes! tracks existing files.
        // Unchanged frontend builds preserve this tree and its file timestamps.
        println!("cargo:rerun-if-changed=../build");
    }
}
