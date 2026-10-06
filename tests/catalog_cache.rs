//! The web server re-reads package indexes for every page; parsing is cached until a file changes.
mod common;
use mibpkg::catalog;
use std::fs;

#[test]
fn indexes_are_parsed_once_until_the_file_changes() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 1", "");
    let dir = common::collection(&f.sd(), "complex256", &[src]);
    let index = dir.join("index.json");
    let first = catalog::collections(&f.layout);
    assert_eq!(first.len(), 1);

    // Same size and modification time: the cached parse is used (garbage is never read).
    let original = fs::read(&index).unwrap();
    let mtime = fs::metadata(&index).unwrap().modified().unwrap();
    fs::write(&index, vec![b' '; original.len()]).unwrap();
    fs::File::options().write(true).open(&index).unwrap().set_modified(mtime).unwrap();
    assert_eq!(catalog::collections(&f.layout).len(), 1, "served from the cache");

    // A real change (different size) is picked up.
    fs::write(&index, b"{}").unwrap();
    assert!(catalog::collections(&f.layout).is_empty(), "the broken index is read again and skipped");
    fs::write(&index, &original).unwrap();
    assert_eq!(catalog::collections(&f.layout).len(), 1);
}
