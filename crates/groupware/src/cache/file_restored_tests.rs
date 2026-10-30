/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

const MISSING_DOCUMENT_ID: u32 = 9;

fn folder(document_id: u32, name: &str, parent_id: Option<u32>) -> DavResource {
    DavResource {
        document_id,
        data: DavResourceMetadata::File {
            name: name.to_string(),
            size: None,
            parent_id,
            acls: Default::default(),
        },
    }
}

fn file(document_id: u32, name: &str, parent_id: Option<u32>) -> DavResource {
    DavResource {
        document_id,
        data: DavResourceMetadata::File {
            name: name.to_string(),
            size: Some(1024),
            parent_id,
            acls: Default::default(),
        },
    }
}

fn build(resources: Vec<DavResource>) -> DavResources {
    let mut files = DavResources {
        base_path: "/dav/file/john/".to_string(),
        paths: AHashSet::with_capacity(resources.len()),
        resources,
        item_change_id: 0,
        container_change_id: 0,
        highest_change_id: 0,
        size: 0,
        update_lock: Arc::new(UpdateLock::new()),
    };
    build_nested_hierarchy(&mut files);
    files
}

fn sorted_paths(files: &DavResources) -> Vec<&str> {
    let mut paths = files
        .paths
        .iter()
        .map(|path| path.path.as_str())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

fn hierarchy_seq(files: &DavResources, path: &str) -> u32 {
    files.paths.get(path).expect(path).hierarchy_seq
}

#[test]
fn nested_hierarchy() {
    let files = build(vec![
        folder(0, "docs", None),
        folder(1, "reports", Some(0)),
        file(2, "q1.txt", Some(1)),
        file(3, "readme.txt", None),
    ]);

    assert_eq!(
        sorted_paths(&files),
        ["docs", "docs/reports", "docs/reports/q1.txt", "readme.txt"]
    );
    assert!(hierarchy_seq(&files, "docs") < hierarchy_seq(&files, "docs/reports"));
    assert!(
        hierarchy_seq(&files, "docs/reports") < hierarchy_seq(&files, "docs/reports/q1.txt")
    );
}

#[test]
fn nested_hierarchy_percent_encodes_paths() {
    let files = build(vec![
        folder(0, "My Documents", None),
        folder(1, "Berichte 2026", Some(0)),
        file(2, "Ünterlagen Q1.txt", Some(1)),
        folder(3, "My%20Folder", None),
        file(4, "file(1)+a:b.txt", Some(3)),
    ]);

    assert_eq!(
        sorted_paths(&files),
        [
            "My%20Documents",
            "My%20Documents/Berichte%202026",
            "My%20Documents/Berichte%202026/%C3%9Cnterlagen%20Q1.txt",
            "My%20Folder",
            "My%20Folder/file(1)+a:b.txt",
        ]
    );
    assert_eq!(
        files.format_resource(files.by_path("My%20Documents").unwrap()),
        "/dav/file/john/My%20Documents/"
    );
    assert_eq!(
        files.format_resource(
            files
                .by_path("My%20Documents/Berichte%202026/%C3%9Cnterlagen%20Q1.txt")
                .unwrap()
        ),
        "/dav/file/john/My%20Documents/Berichte%202026/%C3%9Cnterlagen%20Q1.txt"
    );
}

#[test]
fn nested_hierarchy_with_dangling_parent() {
    let files = build(vec![
        folder(0, "docs", None),
        folder(1, "reports", Some(MISSING_DOCUMENT_ID)),
        file(2, "q1.txt", Some(1)),
        file(3, "orphan.txt", Some(MISSING_DOCUMENT_ID)),
    ]);

    assert_eq!(
        sorted_paths(&files),
        ["docs", "orphan.txt", "reports", "reports/q1.txt"]
    );
    assert!(hierarchy_seq(&files, "reports") < hierarchy_seq(&files, "reports/q1.txt"));
}
