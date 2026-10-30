/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

const INDEX: &str = concat!(
    "<!doctype html>\n<html lang=\"en\">\n\n<head>\n  <meta charset=\"UTF-8\" />\n",
    "  <base href=\"/\" />\n  <meta name=\"oauth-client-id\" content=\"\" />\n",
    "  <title>Portal</title>\n</head>\n\n<body></body>\n\n</html>\n"
);

#[test]
fn index_is_rewritten_with_the_prefix_and_client_id() {
    let meta = oauth_client_id_meta("stalwart-webui");
    let html = String::from_utf8(rewrite_index(INDEX, "admin", Some(&meta))).unwrap();

    assert!(html.contains("<base href=\"/admin/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"stalwart-webui\" />"),
        "{html}"
    );
    assert!(html.contains("<title>Portal</title>"), "{html}");
    assert!(html.starts_with("<!doctype html>"), "{html}");
    assert!(html.ends_with("</html>\n"), "{html}");
}

#[test]
fn index_keeps_the_empty_placeholder_when_no_client_id_is_configured() {
    let html = String::from_utf8(rewrite_index(INDEX, "account", None)).unwrap();

    assert!(html.contains("<base href=\"/account/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"\" />"),
        "{html}"
    );
}

#[test]
fn index_without_a_placeholder_is_left_alone() {
    let bundle = "<head>\n  <base href=\"/\" />\n</head>";
    let meta = oauth_client_id_meta("stalwart-webui");
    let html = String::from_utf8(rewrite_index(bundle, "admin", Some(&meta))).unwrap();

    assert_eq!(html, "<head>\n  <base href=\"/admin/\" />\n</head>");
}

#[test]
fn edits_are_applied_in_document_order() {
    let bundle = concat!(
        "<head><meta name=\"oauth-client-id\" content=\"\" />",
        "<base href=\"/\" /></head>"
    );
    let meta = oauth_client_id_meta("app");
    let html = String::from_utf8(rewrite_index(bundle, "admin", Some(&meta))).unwrap();

    assert_eq!(
        html,
        concat!(
            "<head><meta name=\"oauth-client-id\" content=\"app\" />",
            "<base href=\"/admin/\" /></head>"
        )
    );
}

#[test]
fn client_ids_are_escaped_for_the_attribute() {
    let meta = oauth_client_id_meta("a\"b&c<d>");

    assert_eq!(
        meta,
        "<meta name=\"oauth-client-id\" content=\"a&quot;b&amp;c&lt;d&gt;\""
    );
}

async fn fixture(name: &str, client_id: Option<&str>) -> WebApplications {
    let dir = TempDir::new(std::env::temp_dir().join(format!("stalwart-app-{name}")));
    dir.create().await.unwrap();
    tokio::fs::write(dir.path.join("index.html"), INDEX)
        .await
        .unwrap();
    tokio::fs::write(dir.path.join("app.js"), "export const x = 1;\n")
        .await
        .unwrap();

    let mut resources = AHashMap::new();
    resources.insert(
        "index.html".to_string(),
        Resource::new("text/html", dir.path.join("index.html")),
    );
    resources.insert(
        "app.js".to_string(),
        Resource::new("text/javascript", dir.path.join("app.js")),
    );

    // 1.0 renders each prefix's index page when the bundle is unpacked.
    let routes = Arc::new(AppRoutes {
        resources,
        index_pages: index_pages(
            INDEX.as_bytes(),
            &["admin".to_string(), "account".to_string()],
            client_id,
        ),
        oauth_client: None,
        _bundle_dir: dir,
    });

    let mut map = AHashMap::new();
    map.insert("admin".to_string(), routes.clone());
    map.insert("account".to_string(), routes);

    let apps = WebApplications::new();
    apps.routes.store(Arc::new(map));

    apps
}

async fn serve_html(apps: &WebApplications, prefix: &str, path: &str) -> String {
    let served = apps.serve(prefix, path).await.unwrap().unwrap();
    assert!(served.no_cache, "index responses must not be cached");
    assert_eq!(served.resource.content_type.as_ref(), "text/html");
    String::from_utf8(served.resource.contents.to_vec()).unwrap()
}

#[tokio::test]
async fn serving_index_injects_the_prefix_and_client_id() {
    let apps = fixture("serve-configured", Some("pocket-id-client")).await;

    let html = serve_html(&apps, "admin", "index.html").await;
    assert!(html.contains("<base href=\"/admin/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"pocket-id-client\" />"),
        "{html}"
    );

    let html = serve_html(&apps, "account", "index.html").await;
    assert!(html.contains("<base href=\"/account/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"pocket-id-client\" />"),
        "{html}"
    );
}

#[tokio::test]
async fn unknown_paths_fall_back_to_a_rewritten_index() {
    let apps = fixture("serve-fallback", Some("pocket-id-client")).await;

    let html = serve_html(&apps, "admin", "settings/directory").await;
    assert!(html.contains("<base href=\"/admin/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"pocket-id-client\" />"),
        "{html}"
    );
}

#[tokio::test]
async fn assets_and_unknown_prefixes_are_untouched() {
    let apps = fixture("serve-assets", Some("pocket-id-client")).await;

    let served = apps.serve("admin", "app.js").await.unwrap().unwrap();
    assert_eq!(served.resource.contents, &b"export const x = 1;\n"[..]);
    assert_eq!(served.resource.content_type.as_ref(), "text/javascript");
    assert!(!served.no_cache);

    assert!(apps.serve("unknown", "index.html").await.unwrap().is_none());
}

#[tokio::test]
async fn serving_index_without_a_client_id_keeps_the_placeholder() {
    let apps = fixture("serve-unconfigured", None).await;

    let html = serve_html(&apps, "admin", "index.html").await;
    assert!(html.contains("<base href=\"/admin/\" />"), "{html}");
    assert!(
        html.contains("<meta name=\"oauth-client-id\" content=\"\" />"),
        "{html}"
    );
}

#[test]
fn an_unmodified_document_is_returned_verbatim() {
    let bundle = "<head><title>x</title></head>";

    assert_eq!(rewrite_index(bundle, "admin", None), bundle.as_bytes());
}
#[tokio::test]
async fn missing_parent_directories_are_created() {
    let base = std::env::temp_dir().join("stalwart-app-nested");
    let _ = tokio::fs::remove_dir_all(&base).await;

    let dir = TempDir::new(base.join("webui").join("0"));
    dir.create().await.unwrap();

    assert!(tokio::fs::metadata(&dir.path).await.is_ok());

    drop(dir);
    let _ = tokio::fs::remove_dir_all(&base).await;
}

#[tokio::test]
async fn dropping_the_routes_removes_the_bundle_directory() {
    let apps = fixture("drop-guard", None).await;
    let path = apps
        .routes
        .load()
        .get("admin")
        .unwrap()
        ._bundle_dir
        .path
        .clone();

    assert!(tokio::fs::metadata(&path).await.is_ok());

    apps.routes.store(Arc::new(AHashMap::new()));

    assert!(tokio::fs::metadata(&path).await.is_err());
}

#[tokio::test]
async fn sweeping_orphans_spares_the_current_generation() {
    let base = std::env::temp_dir().join("stalwart-app-sweep");
    let _ = tokio::fs::remove_dir_all(&base).await;

    let current = TempDir::new(base.join("1"));
    current.create().await.unwrap();
    let orphan = base.join("0");
    tokio::fs::create_dir_all(&orphan).await.unwrap();
    let stray = base.join("webui.zip");
    tokio::fs::write(&stray, b"not a bundle").await.unwrap();

    remove_siblings(&base, &current.path).await;

    assert!(tokio::fs::metadata(&current.path).await.is_ok());
    assert!(tokio::fs::metadata(&orphan).await.is_err());
    assert!(tokio::fs::metadata(&stray).await.is_err());

    drop(current);
    let _ = tokio::fs::remove_dir_all(&base).await;
}

#[test]
fn generations_never_repeat() {
    let apps = WebApplications::new();

    assert_ne!(apps.next_generation(), apps.next_generation());
}

fn bundle(version: Option<&str>) -> zip::ZipArchive<Cursor<Vec<u8>>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("index.html", options).unwrap();
    io::Write::write_all(&mut writer, INDEX.as_bytes()).unwrap();
    if let Some(version) = version {
        writer.start_file(APP_MANIFEST, options).unwrap();
        io::Write::write_all(
            &mut writer,
            format!("{{\"id\":\"webui\",\"version\":\"{version}\"}}").as_bytes(),
        )
        .unwrap();
    }
    zip::ZipArchive::new(writer.finish().unwrap()).unwrap()
}

// 1.0 replaced the VERSION file with the `stalwart-app.json` manifest and
// requires the web UI's major version 2 (`verify_manifest`); a bundle
// without a manifest is refused.
#[test]
fn bundles_newer_than_the_supported_major_version_are_rejected() {
    let url = "https://example.org/webui.zip";
    assert!(verify_manifest(&mut bundle(None), url).is_err());
    assert!(verify_manifest(&mut bundle(Some("2.0.12")), url).is_ok());
    assert!(verify_manifest(&mut bundle(Some("1.0.12")), url).is_err());
    assert!(verify_manifest(&mut bundle(Some("3.0.0")), url).is_err());
    assert!(verify_manifest(&mut bundle(Some("10.1.0")), url).is_err());
    assert!(verify_manifest(&mut bundle(Some("garbage")), url).is_err());
}
