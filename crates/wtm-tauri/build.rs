use std::path::Path;

fn main() {
    // `generate_context!` embeds the built frontend and refuses to compile
    // without it. A checkout where nobody has run `bun run build` in `ui/`
    // still builds and tests the Rust side: the window then says what to do.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(&dist).expect("ui/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>Worktree Manager</title>\
             <body style=\"font:14px system-ui;padding:2em\">\
             The frontend is not built. Run <code>bun install &amp;&amp; bun run build</code> in <code>ui/</code>.\
             <script>window.__TAURI_INTERNALS__?.invoke('ready')</script>",
        )
        .expect("ui/dist/index.html");
    }
    println!("cargo:rerun-if-changed=../../ui/dist");
    tauri_build::build()
}
