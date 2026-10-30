/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::{cache::ChunkAccumulator, file::FileProperties};
use common::storage::dav::CONTAINER_FLAG;
use store::{
    Deserialize, Serialize,
    write::{Archive, ArchiveBytes, Archiver},
};

// Restored from 0.16.27 and adapted to 1.0's resource store (upgrade ruling
// 44, an intended change of the API, not of behaviour): resources are pushed
// from archived `FileNode`s with `push_file`, paths live in a `PathIndex`,
// and a container's `hierarchy_seq` carries `CONTAINER_FLAG`, masked here.

const MISSING_DOCUMENT_ID: u32 = 9;

fn node(name: &str, parent_id: Option<u32>, size: Option<u32>) -> FileNode {
    FileNode {
        // 0 is the root; a parent is stored as its id plus one.
        parent_id: parent_id.map_or(0, |parent_id| parent_id + 1),
        name: name.to_string(),
        content: match size {
            Some(size) => crate::file::FileNodeContent::File(FileProperties {
                size,
                ..Default::default()
            }),
            None => crate::file::FileNodeContent::Directory,
        },
        ..Default::default()
    }
}

fn folder(document_id: u32, name: &str, parent_id: Option<u32>) -> (u32, FileNode) {
    (document_id, node(name, parent_id, None))
}

fn file(document_id: u32, name: &str, parent_id: Option<u32>) -> (u32, FileNode) {
    (document_id, node(name, parent_id, Some(1024)))
}

fn build(resources: Vec<(u32, FileNode)>) -> GroupwareResources {
    let mut nodes = ChunkAccumulator::default();
    for (document_id, node) in resources {
        let bytes = Archiver::new(node).serialize().unwrap();
        let archive = <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes).unwrap();
        push_file(
            nodes.current(),
            archive.unarchive::<FileNode>().unwrap(),
            document_id,
        );
    }
    let resources = ResourceStore::from_sorted(nodes.finish(), Vec::new(), true);
    let paths = build_nested_hierarchy(&resources);
    GroupwareResources {
        base_path: "/dav/file/john/".to_string(),
        paths: Arc::new(paths),
        resources,
        item_change_id: 0,
        container_change_id: 0,
        highest_change_id: 0,
        size: 0,
        update_lock: Arc::new(UpdateLock::new()),
        verification: Default::default(),
    }
}

fn sorted_paths(files: &GroupwareResources) -> Vec<&str> {
    let mut paths = files
        .paths
        .iter()
        .map(|(chunk, path)| chunk.path_str(path))
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

fn hierarchy_seq(files: &GroupwareResources, path: &str) -> u32 {
    files.paths.get(path).expect(path).1.hierarchy_seq & !CONTAINER_FLAG
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
    assert!(hierarchy_seq(&files, "docs/reports") < hierarchy_seq(&files, "docs/reports/q1.txt"));
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
            // Ruling 44, an intended 1.0 change: `canonical_path_segment`
            // always percent-encodes a stored name, so a literal `%20` in a
            // name is encoded again; 0.16 passed an encoded segment through.
            "My%2520Folder",
            "My%2520Folder/file(1)+a:b.txt",
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
