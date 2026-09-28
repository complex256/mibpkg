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

### Result (on the MHI2Q, 2026-09-28)

- `dlopen("libcurl.so.2")` works (with `LD_LIBRARY_PATH=/mnt/app/armle/usr/lib`):
  `libcurl/7.56.0 OpenSSL/1.0.2j zlib/1.2.3 c-ares/1.12.0`.
- Plain run: `rc=60 SSL certificate problem: certificate is not yet valid` — the system clock is
  1970. Tailscale's `libtsclock.so` shim does not help: OpenSSL 1.0.2 checks against `time()`,
  which the shim does not interpose.
- **Fix that works:** `CURLOPT_SSL_CTX_FUNCTION` callback → `SSL_CTX_get0_param(ctx)` (libssl) →
  `X509_VERIFY_PARAM_set_time(param, time() + offset)` (libcrypto), offset = network time − system
  time (here from `/tmp/ts_clock_offset`). With it, full verification against the macOS root bundle:
  `https://github.com` → 200 (576 KB), `https://objects.githubusercontent.com/` → 404 (TLS fine),
  `https://www.cloudflare.com/` → 200.
- Consequence for online updates: mibpkg needs its own network-time offset (a small SNTP query over
  UDP in std Rust, or reuse Tailscale's offset file when present) and sets the verify time per
  request; the system clock is never touched.
