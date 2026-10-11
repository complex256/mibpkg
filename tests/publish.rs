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
