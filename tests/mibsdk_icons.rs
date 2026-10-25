//! Icon PNG rules (`icons::png_check`), on the vectors shared with the SDK build's `PngCheckTest`
//! (`tests/fixtures/icons/`, byte copies of the SDK repo's `build-logic/src/test/resources/png/`),
//! plus in-memory vectors for the rules added by the controller ruling, and `check_all`.
use flate2::write::ZlibEncoder;
use flate2::{Compression, Crc};
use mibpkg::mibsdk::icons;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/icons");

fn fixture(name: &str) -> Vec<u8> {
    fs::read(Path::new(FIX).join(name)).unwrap()
}

#[test]
fn png_check_refuses_every_malformed_or_oversized_file() {
    assert_eq!(icons::png_check(&fixture("ok-20.png"), 72).unwrap(), (20, 20));
    assert_eq!(icons::png_check(&fixture("ok-72.png"), 72).unwrap(), (72, 72));
    for bad in ["empty.png", "too-big.png", "truncated.png", "bad-crc.png", "palette.png", "rgb.png",
                "depth16.png", "interlaced.png", "apng.png", "73x20.png", "1x100000.png", "ihdr-not-first.png",
                "iend-not-last.png", "short-idat.png", "compression1.png", "filter1.png", "idat-bomb.png"] {
        assert!(icons::png_check(&fixture(bad), 72).is_err(), "{bad}");
    }
    assert!(fixture("idat-bomb.png").len() < 32768); // refused by the inflate cap
}

#[test]
fn png_check_names_the_rule_each_fixture_breaks() {
    for (bad, want) in [
        ("empty.png", "no PNG signature"),
        ("too-big.png", "more than 32768"),
        ("truncated.png", "truncated"),
        ("bad-crc.png", "bad CRC"),
        ("palette.png", "colour type 3"),
        ("rgb.png", "colour type 2"),
        ("depth16.png", "bit depth 16"),
        ("interlaced.png", "interlace 1"),
        ("apng.png", "animated PNG"),
        ("73x20.png", "size 73x20"),
        ("1x100000.png", "size 1x100000"),
        ("ihdr-not-first.png", "not IHDR"),
        ("compression1.png", "compression 1"),
        ("filter1.png", "filter 1"),
        ("idat-bomb.png", "more than the expected"),
    ] {
        let e = format!("{:#}", icons::png_check(&fixture(bad), 72).unwrap_err());
        assert!(e.contains(want), "{bad}: {e}");
    }
    // max_px is the caller's: a 72x72 icon is refused where at most 20 is allowed.
    assert!(icons::png_check(&fixture("ok-72.png"), 20).is_err());
}

// ---- in-memory vectors ----

fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = Crc::new();
    c.update(ty);
    c.update(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    out.extend(c.sum().to_be_bytes());
    out
}

fn ihdr(w: u32, h: u32) -> Vec<u8> {
    let mut d = w.to_be_bytes().to_vec();
    d.extend(h.to_be_bytes());
    d.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &d)
}

/// Raw scanlines of a w x h RGBA image, every row with filter type `filter`.
fn rows(w: u32, h: u32, filter: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..h {
        out.push(filter);
        for x in 0..w * 4 {
            out.push((x + y) as u8);
        }
    }
    out
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    for c in chunks {
        out.extend_from_slice(c);
    }
    out
}

fn refusal(bytes: &[u8]) -> String {
    format!("{:#}", icons::png_check(bytes, 72).expect_err("accepted"))
}

#[test]
fn in_memory_vectors_are_valid_as_built() {
    let z = zlib(&rows(4, 3, 0));
    assert_eq!(icons::png_check(&png(&[ihdr(4, 3), chunk(b"IDAT", &z), chunk(b"IEND", b"")]), 72).unwrap(), (4, 3));
    // IDAT split over consecutive chunks is fine; so are ancillary chunks before and after them.
    let (a, b) = z.split_at(z.len() / 2);
    let ok = png(&[ihdr(4, 3), chunk(b"tEXt", b"k\0v"), chunk(b"IDAT", a), chunk(b"IDAT", b), chunk(b"tEXt", b"k\0v"), chunk(b"IEND", b"")]);
    assert_eq!(icons::png_check(&ok, 72).unwrap(), (4, 3));
    // Every filter type 0..4 is accepted.
    assert!(icons::png_check(&png(&[ihdr(4, 3), chunk(b"IDAT", &zlib(&rows(4, 3, 4))), chunk(b"IEND", b"")]), 72).is_ok());
}

#[test]
fn png_check_refuses_a_second_ihdr() {
    let z = zlib(&rows(4, 3, 0));
    let e = refusal(&png(&[ihdr(4, 3), ihdr(4, 3), chunk(b"IDAT", &z), chunk(b"IEND", b"")]));
    assert!(e.contains("second IHDR"), "{e}");
}

#[test]
fn png_check_refuses_non_consecutive_idat_chunks() {
    let z = zlib(&rows(4, 3, 0));
    let (a, b) = z.split_at(z.len() / 2);
    let e = refusal(&png(&[ihdr(4, 3), chunk(b"IDAT", a), chunk(b"tEXt", b"k\0v"), chunk(b"IDAT", b), chunk(b"IEND", b"")]));
    assert!(e.contains("not consecutive"), "{e}");
}

#[test]
fn png_check_refuses_a_row_filter_above_4() {
    let mut raw = rows(4, 3, 0);
    raw[2 * (1 + 16)] = 5; // the last row
    let e = refusal(&png(&[ihdr(4, 3), chunk(b"IDAT", &zlib(&raw)), chunk(b"IEND", b"")]));
    assert!(e.contains("row 2 has filter type 5"), "{e}");
}

#[test]
fn png_check_refuses_data_after_the_zlib_stream_end() {
    let mut z = zlib(&rows(4, 3, 0));
    z.extend([0, 0]);
    let e = refusal(&png(&[ihdr(4, 3), chunk(b"IDAT", &z), chunk(b"IEND", b"")]));
    assert!(e.contains("after the end of the zlib stream"), "{e}");
}

#[test]
fn png_check_refuses_an_unfinished_zlib_stream_with_the_right_byte_count() {
    let raw = rows(4, 3, 0);
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(&raw).unwrap();
    e.flush().unwrap(); // a sync flush: every byte decodable, but no final block and no Adler-32
    let z = e.get_ref().clone();
    let mut d = flate2::Decompress::new(true);
    let mut out = Vec::with_capacity(raw.len() + 1);
    d.decompress_vec(&z, &mut out, flate2::FlushDecompress::None).unwrap();
    assert_eq!(out, raw, "the vector must inflate to exactly the expected bytes");
    let e = refusal(&png(&[ihdr(4, 3), chunk(b"IDAT", &z), chunk(b"IEND", b"")]));
    assert!(e.contains("not a complete zlib stream"), "{e}");
}

#[test]
fn png_check_refuses_any_byte_after_iend() {
    let mut b = fixture("ok-20.png");
    b.push(0);
    assert!(refusal(&b).contains("data after IEND"));
}

// ---- check_all ----

#[test]
fn check_all_reads_checks_and_hashes_every_declared_icon_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("icons")).unwrap();
    fs::write(dir.path().join("icons/hello.png"), fixture("ok-72.png")).unwrap();
    fs::write(dir.path().join("icons/small.png"), fixture("ok-20.png")).unwrap();
    let before: Vec<_> = fs::read_dir(dir.path().join("icons")).unwrap().map(|e| e.unwrap().path()).collect();
    let names = vec!["small".to_string(), "hello".to_string()];
    let c = icons::check_all(dir.path(), &names).unwrap();
    let want: String = Sha256::digest(fixture("ok-72.png")).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(c[1].sha16, want[..16]);
    assert_eq!((c[1].w, c[1].h, c[0].w, c[0].h), (72, 72, 20, 20));
    assert_eq!(c[1].bytes, fixture("ok-72.png"));
    assert_eq!(
        icons::index_text(&c),
        format!("hello={},72,72\nsmall={},20,20\n", c[1].sha16, c[0].sha16)
    );
    let after: Vec<_> = fs::read_dir(dir.path().join("icons")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(before.len(), after.len());

    let e = format!("{:#}", icons::check_all(dir.path(), &["gone".to_string()]).unwrap_err());
    assert!(e.starts_with("icon gone: "), "{e}");
    fs::write(dir.path().join("icons/bad.png"), fixture("bad-crc.png")).unwrap();
    let e = format!("{:#}", icons::check_all(dir.path(), &["bad".to_string()]).unwrap_err());
    assert!(e.starts_with("icon bad: bad CRC"), "{e}");
}

/// The shared vectors for the tightened rules (SDK `6f07811`): each refused for its own reason.
#[test]
fn png_check_refuses_each_tightened_rule_fixture_for_its_reason() {
    for (bad, want) in [
        ("ihdr-twice.png", "second IHDR"),
        ("idat-split.png", "not consecutive"),
        ("row-filter5.png", "has filter type 5"),
        ("idat-trailing.png", "after the end of the zlib stream"),
        ("zlib-unfinished.png", "not a complete zlib stream"),
        ("iend-trailing-byte.png", "data after IEND"),
    ] {
        let b = fixture(bad);
        assert!(b.len() <= 32768, "{bad}");
        let e = format!("{:#}", icons::png_check(&b, 72).unwrap_err());
        assert!(e.contains(want), "{bad}: {e}");
    }
}
