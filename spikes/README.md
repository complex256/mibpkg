# Spikes (throwaway)

Code here answers a feasibility question; it is not part of mibpkg and is not built by `cargo test`.

## curlspike — HTTPS from the unit via its own libcurl (2026-09-26)

Question: can mibpkg fetch over HTTPS on QNX 6.5? Pure-Rust TLS does not build for the target
(`ring`/`aws-lc`: cc-rs "Unknown architecture for Neutrino QNX: arm"; `rustls-rustcrypto`: the
`libc` crate does not compile for nto65/arm). The unit ships libcurl 7.56.0 +
OpenSSL 1.0.2j (TLS 1.2) at /mnt/app/armle/usr/lib; this probe dlopen()s `libcurl.so.2`.

Build: `docker run --rm -v "$PWD/spikes/curlspike":/src/curlspike -w /src qnx65-armv7-toolchain:8.5-rust build-std /src/curlspike`
Run on the unit (car on, `audi` reachable):

```
scp spikes/curlspike/target/armv7-unknown-nto-qnx650/release/curlspike audi:/tmp/
scp /etc/ssl/cert.pem audi:/tmp/ca.pem          # macOS root bundle, for the probe only
ssh -n audi 'LD_LIBRARY_PATH=/mnt/app/armle/usr/lib:$LD_LIBRARY_PATH /tmp/curlspike https://github.com /tmp/ca.pem'
ssh -n audi 'LD_LIBRARY_PATH=/mnt/app/armle/usr/lib:$LD_LIBRARY_PATH /tmp/curlspike https://objects.githubusercontent.com/ /tmp/ca.pem'
```
Expect `rc=0 http=200` (or 30x/404 from the second host — any HTTP code proves TLS works).
Watch for: dlopen failure (library path), cert errors while the unit clock is still 1970
(TLS needs a sane clock), and TLS handshake failures from the 2016 OpenSSL.
