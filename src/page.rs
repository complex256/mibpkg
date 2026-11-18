//! Store pages: the markdown a package carries as its first archive entry, fetched on demand.
use crate::archive::read_page;
use crate::fetch::{Fetcher, MAX_PACKAGE};
use crate::index::{IndexEntry, Location};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// How much of a remote package's head is requested first. The page is at most 64 KiB and
/// gzip adds almost nothing to incompressible text, so this holds the whole first entry.
pub const HEAD_BYTES: u64 = 96 << 10;

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks `page` against what the index recorded and returns it as text.
pub fn verified(e: &IndexEntry, page: Vec<u8>) -> Result<String> {
    let Some(want) = &e.page_sha256 else { bail!("{} {} has no store page", e.name, e.version) };
    if e.page_bytes.is_some_and(|n| n != page.len() as u64) || &hex(&page) != want {
        bail!("the store page of {} {} does not match its index entry", e.name, e.version);
    }
    String::from_utf8(page).context("store page is not UTF-8")
}

/// The page of index entry `e` stored at `loc`. A local archive is read directly. A remote one
/// is first asked for just its head (the page is the first entry); when that fails or does not
/// hold the whole page, the package is downloaded within `MAX_PACKAGE`, checked against the
/// entry's sha256 and read.
pub fn fetch(fetcher: &dyn Fetcher, loc: &Location, e: &IndexEntry) -> Result<String> {
    if e.page_sha256.is_none() {
        bail!("{} {} has no store page", e.name, e.version);
    }
    let url = match loc {
        Location::Local(p) => {
            let f = std::fs::File::open(p).with_context(|| format!("opening {}", p.display()))?;
            let page = read_page(f)?.context("the package holds no store page")?;
            return verified(e, page);
        }
        Location::Remote(u) => u,
    };
    if let Ok(head) = fetcher.get_head(url, HEAD_BYTES) {
        if let Ok(Some(page)) = read_page(head.as_slice()) {
            return verified(e, page);
        }
    }
    let all = fetcher.get(url, MAX_PACKAGE)?;
    if hex(&all) != e.sha256 {
        bail!("{url}: downloaded file does not match its index entry");
    }
    let page = read_page(all.as_slice())?.context("the package holds no store page")?;
    verified(e, page)
}
