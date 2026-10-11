//! Host publisher tooling: mibpkg release.
mod common;
use mibpkg::fetch::MemoryFetcher;
use mibpkg::index::Index;
use mibpkg::publish::{self, Publisher, ReleaseOpts};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

const REPO: &str = "alice/tools";
const LATEST: &str = "https://github.com/alice/tools/releases/latest/download/index.json";

#[derive(Default)]
struct FakeGh {
    tag: Option<String>,
    created: RefCell<Vec<(String, Vec<String>)>>,
}
impl Publisher for FakeGh {
    fn latest_tag(&self, _repo: &str) -> anyhow::Result<Option<String>> {
        Ok(self.tag.clone())
    }
    fn create(&self, _repo: &str, tag: &str, _notes: &str, files: &[PathBuf]) -> anyhow::Result<()> {
        let mut names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        names.sort();
        self.created.borrow_mut().push((tag.into(), names));
        Ok(())
    }
}

fn opts(f: &common::Fixture, inputs: Vec<PathBuf>, tag: &str) -> ReleaseOpts {
    ReleaseOpts {
        inputs,
        repo: REPO.into(),
        tag: tag.into(),
        id: Some("alice-tools".into()),
        name: Some("Alice's tools".into()),
        new_id: false,
        keep: 3,
        out: f.root().join(format!("out-{tag}")),
        assets: vec![],
        publish: false,
    }
}

/// Serve a release's index as the repo's latest.
fn serve(m: &MemoryFetcher, index: &Index) {
    m.files.borrow_mut().insert(LATEST.into(), serde_json::to_vec(index).unwrap());
}

#[test]
fn first_release_builds_and_links_absolute_urls() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let gh = FakeGh::default();
    let r = publish::release(&opts(&f, vec![src], "v1"), &MemoryFetcher::default(), &gh).unwrap();
    assert_eq!(r.index.collection.id, "alice-tools");
    assert_eq!(r.index.packages.len(), 1);
    assert_eq!(r.index.packages[0].file, "https://github.com/alice/tools/releases/download/v1/demo-1.0.0.mpk");
    assert!(f.root().join("out-v1/index.json").is_file());
    assert!(f.root().join("out-v1/demo-1.0.0.mpk").is_file());
    assert!(gh.created.borrow().is_empty(), "no --publish, nothing uploaded");
}

#[test]
fn later_releases_carry_old_versions_and_keep_the_newest_n() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    for (i, v) in ["1.0.0", "1.1.0", "1.2.0", "1.3.0"].iter().enumerate() {
        let src = common::pkg_src(f.root(), "demo", v, "sleep 30", "");
        let mut o = opts(&f, vec![src], &format!("v{i}"));
        o.publish = true;
        let r = publish::release(&o, &m, &gh).unwrap();
        serve(&m, &r.index);
    }
    let last = gh.created.borrow().last().unwrap().clone();
    assert_eq!(last.0, "v3");
    assert_eq!(last.1, vec!["demo-1.3.0.mpk".to_string(), "index.json".into()], "only new files are uploaded");
    let r = publish::release(&opts(&f, vec![], "v4"), &m, &gh).unwrap();
    let versions: Vec<&str> = r.index.packages.iter().map(|e| e.version.as_str()).collect();
    assert_eq!(versions, ["1.1.0", "1.2.0", "1.3.0"], "keep 3 newest");
    assert!(r.index.packages[0].file.ends_with("/v1/demo-1.1.0.mpk"));
}

#[test]
fn a_reused_version_with_new_contents_is_refused_before_uploading() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    serve(&m, &publish::release(&opts(&f, vec![src], "v1"), &m, &gh).unwrap().index);
    let changed = common::pkg_src(&f.root().join("b"), "demo", "1.0.0", "sleep 99", "");
    let mut o = opts(&f, vec![changed], "v2");
    o.publish = true;
    let e = publish::release(&o, &m, &gh).unwrap_err().to_string();
    assert!(e.contains("1.0.0") && e.contains("different contents"), "{e}");
    assert!(gh.created.borrow().is_empty());
    // The identical file again is fine (kept once, not re-uploaded). Reuse the built file: packing
    // the same directory twice need not give identical bytes.
    let same = f.root().join("out-v1/demo-1.0.0.mpk");
    let r = publish::release(&opts(&f, vec![same], "v3"), &m, &gh).unwrap();
    assert_eq!(r.index.packages.len(), 1);
    assert!(r.index.packages[0].file.ends_with("/v1/demo-1.0.0.mpk"));
}

#[test]
fn the_collection_id_cannot_change_by_accident() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    serve(&m, &publish::release(&opts(&f, vec![src], "v1"), &m, &gh).unwrap().index);
    let mut o = opts(&f, vec![], "v2");
    o.id = Some("other".into());
    assert!(publish::release(&o, &m, &gh).unwrap_err().to_string().contains("--new-id"));
    o.new_id = true;
    assert_eq!(publish::release(&o, &m, &gh).unwrap().index.collection.id, "other");
    // Without --id a later release keeps the published id; a first release needs one.
    let mut o = opts(&f, vec![], "v3");
    o.id = None;
    assert_eq!(publish::release(&o, &m, &gh).unwrap().index.collection.id, "alice-tools");
    let mut o = opts(&f, vec![], "v1");
    o.id = None;
    assert!(publish::release(&o, &MemoryFetcher::default(), &gh).unwrap_err().to_string().contains("--id"));
}

#[test]
fn relative_entries_from_older_indexes_become_absolute() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh { tag: Some("v2".into()), ..Default::default() };
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let mut idx = publish::release(&opts(&f, vec![src], "v2"), &m, &gh).unwrap().index;
    idx.packages[0].file = "demo-1.0.0.mpk".into(); // a 0.3.x-era flat index
    serve(&m, &idx);
    let r = publish::release(&opts(&f, vec![], "v3"), &m, &gh).unwrap();
    assert_eq!(r.index.packages[0].file, "https://github.com/alice/tools/releases/download/v2/demo-1.0.0.mpk");
}

#[test]
fn a_rerun_reuses_its_own_out_dir_and_assets_ride_along() {
    let f = common::fixture();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let asset = f.root().join("sources.json");
    fs::write(&asset, "{}").unwrap();
    let mut o = opts(&f, vec![src], "v1");
    o.assets = vec![asset];
    publish::release(&o, &MemoryFetcher::default(), &gh).unwrap();
    let r = publish::release(&o, &MemoryFetcher::default(), &gh).unwrap();
    let mut names: Vec<String> = r.files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
    names.sort();
    assert_eq!(names, ["demo-1.0.0.mpk", "index.json", "sources.json"]);
    // An out dir holding anything else is never wiped.
    fs::write(o.out.join("notes.txt"), "mine").unwrap();
    assert!(publish::release(&o, &MemoryFetcher::default(), &gh).unwrap_err().to_string().contains("not a release output"));
}

#[test]
fn wait_live_sees_the_new_index() {
    let m = MemoryFetcher::default();
    m.files.borrow_mut().insert(LATEST.into(), b"new".to_vec());
    let d = std::time::Duration::from_millis(1);
    assert!(publish::wait_live(&m, LATEST, b"new", d, d));
    assert!(!publish::wait_live(&m, LATEST, b"other", d, d));
}

/// A published release served by a MemoryFetcher: index + package bytes at their URLs.
fn published_release(f: &common::Fixture, versions: &[&str]) -> (MemoryFetcher, Index) {
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let mut index = None;
    for (i, v) in versions.iter().enumerate() {
        let src = common::pkg_src(f.root(), "demo", v, "sleep 30", "");
        let tag = format!("v{i}");
        let o = opts(f, vec![src], &tag);
        let r = publish::release(&o, &m, &gh).unwrap();
        for p in &r.files {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            if n.ends_with(".mpk") {
                m.files.borrow_mut().insert(publish::download_url(REPO, &tag, &n), fs::read(p).unwrap());
            }
        }
        serve(&m, &r.index);
        index = Some(r.index);
    }
    (m, index.unwrap())
}

#[test]
fn verify_passes_a_good_source() {
    let f = common::fixture();
    let (m, _) = published_release(&f, &["1.0.0", "1.1.0"]);
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, 2);
    assert_eq!(publish::verify("github:alice/tools", true, None, &m).unwrap().checked, 1);
}

#[test]
fn verify_reports_every_problem() {
    let f = common::fixture();
    let (m, mut idx) = published_release(&f, &["1.0.0", "1.1.0"]);
    idx.packages[0].sha256 = "0".repeat(64); // 1.0.0: wrong hash
    idx.packages[1].version = "1.2.0".into(); // 1.1.0's file claims 1.2.0
    serve(&m, &idx);
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
    assert!(r.problems.iter().any(|p| p.contains("sha256")), "{:?}", r.problems);
    assert!(r.problems.iter().any(|p| p.contains("holds demo 1.1.0")), "{:?}", r.problems);
}

#[test]
fn verify_reports_unreachable_files_and_bad_indexes() {
    let f = common::fixture();
    let (m, idx) = published_release(&f, &["1.0.0"]);
    m.files.borrow_mut().retain(|k, _| k.ends_with("index.json"));
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert!(r.problems[0].contains("404"), "{:?}", r.problems);
    let mut bad = idx.clone();
    bad.packages[0].file = "http://insecure/demo-1.0.0.mpk".into();
    serve(&m, &bad);
    assert!(publish::verify("github:alice/tools", false, None, &m).unwrap().problems[0].contains("https"));
    assert!(publish::verify("github:nobody/none", false, None, &m).is_err());
}

#[test]
fn verify_checks_a_local_release_dir_before_publishing() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let o = opts(&f, vec![src], "v1");
    publish::release(&o, &MemoryFetcher::default(), &FakeGh::default()).unwrap();
    let r = publish::verify(o.out.to_str().unwrap(), false, None, &MemoryFetcher::default()).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, 1);
}

#[test]
fn verify_notes_entries_another_unit_cannot_use() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "[compat]\nunits = [\"MHI2\"]\n");
    let o = opts(&f, vec![src], "v1");
    publish::release(&o, &MemoryFetcher::default(), &FakeGh::default()).unwrap();
    let unit = mibpkg::unit::UnitInfo { unit: Some("MHI2Q".into()), firmware: None };
    let r = publish::verify(o.out.to_str().unwrap(), false, Some(&unit), &MemoryFetcher::default()).unwrap();
    assert!(r.problems.is_empty());
    assert!(r.notes[0].contains("not for MHI2Q"), "{:?}", r.notes);
}

#[test]
fn new_scaffolds_a_package_that_builds() {
    let f = common::fixture();
    let dir = f.root().join("mytool");
    let files = mibpkg::scaffold::new_package(&dir, "mytool").unwrap();
    assert!(files.iter().any(|p| p.ends_with(".github/workflows/mibpkg-release.yml")));
    fs::write(dir.join("bin/mytool"), "#!/bin/sh\nsleep 60\n").unwrap();
    let m = mibpkg::archive::pack(&dir, &f.root().join("mytool.mpk")).unwrap();
    assert_eq!((m.package.name.as_str(), m.package.version.as_str()), ("mytool", "0.1.0"));
    assert!(mibpkg::scaffold::new_package(&dir, "mytool").unwrap_err().to_string().contains("not empty"));
    assert!(mibpkg::scaffold::new_package(&f.root().join("x"), "Bad_Name").is_err());
}

#[test]
fn the_committed_source_list_is_valid() {
    let text = fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/sources.json")).unwrap();
    let p = mibpkg::sourcelist::parse_list(&text).unwrap();
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    assert!(p.entries.iter().any(|e| e.source == mibpkg::online::DEFAULT_SOURCE));
}

#[test]
fn a_new_version_that_keep_would_drop_is_refused() {
    let f = common::fixture();
    let gh = FakeGh::default();
    let a = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let b = common::pkg_src(f.root(), "demo", "2.0.0", "sleep 30", "");
    let mut o = opts(&f, vec![a, b], "v1");
    o.keep = 1;
    o.publish = true;
    let e = publish::release(&o, &MemoryFetcher::default(), &gh).unwrap_err().to_string();
    assert!(e.contains("demo 1.0.0") && e.contains("--keep"), "{e}");
    assert!(gh.created.borrow().is_empty());
}

#[test]
fn an_older_version_added_after_a_newer_one_counts_correctly() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let b = common::pkg_src(f.root(), "demo", "2.0.0", "sleep 30", "");
    serve(&m, &publish::release(&opts(&f, vec![b], "v1"), &m, &gh).unwrap().index);
    let a = common::pkg_src(&f.root().join("hotfix"), "demo", "1.0.1", "sleep 30", "");
    let r = publish::release(&opts(&f, vec![a], "v2"), &m, &gh).unwrap();
    assert_eq!((r.added.len(), r.kept), (1, 1));
}

#[test]
fn a_tag_that_looks_like_an_option_is_refused() {
    let f = common::fixture();
    let e = publish::release(&opts(&f, vec![], "--draft"), &MemoryFetcher::default(), &FakeGh::default()).unwrap_err().to_string();
    assert!(e.contains("tag"), "{e}");
}

#[test]
fn verify_checks_an_sd_card_collection() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let dir = common::collection(&f.sd(), "complex256-demo", &[src]);
    let r = publish::verify(dir.to_str().unwrap(), false, None, &MemoryFetcher::default()).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, 1);
}
