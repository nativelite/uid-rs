# uid-rs
Time-sortable unique identifiers — **ULID** and **UUID version 7** (RFC 9562) —
built entirely on the Rust standard library. **Zero dependencies.**

Both formats put a 48-bit millisecond timestamp in their most significant bits,
so **lexicographic order matches creation order** — ideal for database primary
keys and anything you sort by time. Within a single millisecond, IDs stay
**monotonic**: the random field is used as a counter and incremented, so two IDs
minted in the same millisecond still order by creation.

## Why zero dependencies matters here

The usual Rust stack for this is three crates — `uuid`, plus `rand` and its
`getrandom` backend — pulled in for behavior the standard library and the OS
already provide. `uid` takes none of them. Randomness comes straight from the
operating system CSPRNG through `std` alone:

- **Unix:** reads `/dev/urandom` with safe `std::fs` — no `unsafe`, no crate.
- **Windows:** one small FFI call to the system RNG (`RtlGenRandom`, the
  `advapi32` export `SystemFunction036`).

That is the entire "native tools" story: the OS is the dependency you already
trust, so we call it directly instead of vendoring a supply chain.

## Usage

```rust
// A ULID as a 26-character Crockford base32 string.
let id = uid::ulid();                 // "01ARYZ6S41..." (26 chars)

// A UUIDv7, rendered canonically.
let u = uid::uuid7();
assert_eq!(u.version(), 7);
println!("{u}");                      // "0190b2c8-...-7...-8..."

// Same-millisecond IDs still sort by creation order.
let (a, b) = (uid::ulid(), uid::ulid());
assert!(a < b);

// Recover the embedded timestamp.
let ms = uid::ulid_timestamp_ms(&id).unwrap();      // u64 milliseconds
let t  = uid::uuid7().system_time();                // std::time::SystemTime
```

Pin the timestamp (backfills, tests) with `uid::ulid_at(ms)` / `uid::uuid7_at(ms)`.

Command line — the `uid` binary:

```bash
uid            # a new ULID
uid ulid       # a new ULID
uid uuid7      # a new UUID version 7
```

## API

| Item | Returns | Notes |
| --- | --- | --- |
| `ulid()` / `ulid_at(ms)` | `String` | 26-char Crockford base32 |
| `uuid7()` / `uuid7_at(ms)` | `Uuid` | RFC 9562 version 7 |
| `Uuid::version()` | `u8` | `7` for values from `uuid7` |
| `Uuid::timestamp_ms()` / `system_time()` | `u64` / `SystemTime` | embedded 48-bit time |
| `Uuid::as_bytes()` / `from_bytes()` | `&[u8; 16]` / `Uuid` | raw big-endian bytes |
| `ulid_timestamp_ms(s)` / `ulid_system_time(s)` | `Result<_, UlidError>` | parse a ULID string |

`Uuid` derives `Ord` on its raw big-endian bytes, so version-7 values sort by
their embedded timestamp. ID generation **panics** if the OS CSPRNG is
unreadable — an unrecoverable environment failure.

## What's deliberately out of scope

- **UUID v1 / v4 and general UUID parsing.** `uid` mints the two *time-sortable*
  formats; it is not a general UUID library. Its `Uuid` type is a minimal
  value + `Display`, not a full parser.
- **A `rand`-style general RNG.** The OS CSPRNG is used only to seed IDs.

## Correctness

Tests are anchored to the canonical formats, not just round-trips: the ULID
Crockford layout (including the spec example `1469918176385 ms -> "01ARYZ6S41"`),
the UUIDv7 bit layout from RFC 9562 (version, variant, timestamp), and the
monotonic-within-a-millisecond property. This crate mirrors the public behavior
and test vectors of `nativelite/uid-py`.

## Development

```bash
python dev.py check     # zero-dependency guard + cargo test (what CI runs)
python dev.py test      # cargo test (unit + integration + doctests)
python dev.py fmt       # cargo fmt --check
python dev.py guard     # zero-dependency guard
```

`dev.py` is a stdlib-Python runner that drives `cargo`, so `python dev.py check`
is the same one-command local gate used across every nativelite package. The
zero-dependency guard (`tools/dep_guard.py`) fails if `Cargo.toml` declares any
dependency (runtime, build, or dev).
