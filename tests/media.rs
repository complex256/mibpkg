mod common;
use mibpkg::media;
use std::fs;

#[test]
fn finds_collections_errors_and_disable() {
    let f = common::fixture();
    assert_eq!(media::roots(&f.layout), vec![f.sd()]);
    assert!(media::scan(&f.layout).is_empty());
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 1", "");
    common::collection(&f.sd(), "complex256", &[a]);
    fs::create_dir_all(f.sd().join("mibpkg/collections/broken")).unwrap();
    let s = media::scan(&f.layout);
    assert_eq!(s.len(), 2);
    assert!(s[0].1.is_err(), "broken sorts first and reports an error");
    assert_eq!(media::loaded(&s).len(), 1);
    assert!(!media::disabled(&f.layout));
    fs::write(f.sd().join("mibpkg/DISABLE"), "").unwrap();
    assert!(media::disabled(&f.layout));
}

#[test]
fn duplicate_collection_id_first_wins() {
    let f = common::fixture();
    fs::create_dir_all(f.root().join("fs/sdb0")).unwrap();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 1", "");
    let b = common::pkg_src(f.root(), "alpha", "2.0.0", "sleep 1", "");
    common::collection(&f.sd(), "c", &[a]);
    common::collection(&f.root().join("fs/sdb0"), "c", &[b]);
    let l = media::loaded(&media::scan(&f.layout));
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].latest("alpha").unwrap().version, "1.0.0");
}
