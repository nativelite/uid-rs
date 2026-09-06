# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-28

### Added
- `ulid()` / `ulid_at(ms)`: a ULID as a 26-character Crockford base32 string
  (48-bit millisecond timestamp + 80-bit monotonic randomness).
- `uuid7()` / `uuid7_at(ms)`: a UUID version 7 (RFC 9562) as a `Uuid` value
  (48-bit timestamp, 74 random bits acting as a monotonic in-millisecond counter).
- `Uuid` value type: `version()`, `timestamp_ms()`, `system_time()`,
  `as_bytes()`, `from_bytes()`, and canonical hyphenated `Display`. Ordering is
  by the raw big-endian bytes, so version-7 values sort by embedded time.
- `ulid_timestamp_ms()` / `ulid_system_time()` and the `UlidError` type for
  parsing/inspecting ULID strings.
- Zero-dependency OS CSPRNG access: safe `std::fs` reads of `/dev/urandom` on
  Unix, and a minimal `RtlGenRandom` FFI shim on Windows: no `rand`,
  `getrandom`, or `uuid` crate.
- `uid` command-line binary: `uid [ulid|uuid7]`.
- `unittest`-style split test suite (white-box unit tests for the Crockford
  codec and monotonic clock; black-box integration tests for the public API and
  the CLI binary) anchored to the ULID spec example and the RFC 9562 layout.
- Stdlib-Python `dev.py` runner driving cargo and a `Cargo.toml`-based
  zero-dependency guard, matching the nativelite `python dev.py check` gate.

Mirrors the public API and test vectors of `nativelite/uid-py` 0.1.0.

[Unreleased]: https://github.com/nativelite/uid-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nativelite/uid-rs/releases/tag/v0.1.0
