//! The web interface (spec 2026-10-05-web-interface-design.md): `mibpkg web`, started by the
//! supervisor. Reads are served in-process; every write runs `mibpkg gem <verb>` under the lock.
pub mod settings;
pub mod http;
pub mod net;
