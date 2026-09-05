use std::path::PathBuf;

fn workspace_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|err| panic!("failed to read {path}: {err}"))
}

#[test]
fn alpine_compat_is_vendored_and_loaded_before_alpine() {
    let package = workspace_file("package.json");
    assert!(package.contains("dist/ext/hx-alpine-compat.min.js"));
    assert!(package.contains("web/static/vendor/hx-alpine-compat.js"));

    let extension = workspace_file("web/static/vendor/hx-alpine-compat.js");
    assert!(extension.contains("registerExtension(\"alpine-compat\""));

    let base = workspace_file("templates/layouts/base.html");
    let htmx = base.find("vendor/htmx.js").expect("htmx script");
    let compat = base
        .find("vendor/hx-alpine-compat.js")
        .expect("Alpine compatibility extension script");
    let alpine = base
        .find("vendor/alpine.min.js")
        .expect("deferred Alpine script");
    assert!(htmx < compat, "the extension must load after htmx");
    assert!(compat < alpine, "the extension must load before Alpine");
}
