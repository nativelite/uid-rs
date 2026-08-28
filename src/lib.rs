//! uid — time-sortable unique identifiers, standard library only.
//!
//! Two 128-bit ID formats whose lexicographic order matches creation order:
//!
//! * [`ulid`] — a ULID as a 26-character Crockford base32 `String`.
//! * [`uuid7`] — a UUID version 7 (RFC 9562) as a [`Uuid`].
//!
//! Both encode a 48-bit millisecond Unix timestamp in their most significant
//! bits, so sorting the encoded values sorts by time. Within a single
//! millisecond IDs are **monotonic**: the random component is used as a counter
//! and incremented, so two IDs minted in the same millisecond still order by
//! creation.
//!
//! ```
//! let a = uid::ulid();
//! let b = uid::ulid();
//! assert!(a < b);                 // same-ms IDs still order by creation
//! assert_eq!(uid::uuid7().version(), 7);
//! ```
//!
//! # Zero dependencies
//!
//! There are no third-party runtime dependencies. Randomness comes from the
//! operating system CSPRNG, reached through the standard library alone: reading
//! `/dev/urandom` on Unix (safe `std::fs`), and a tiny FFI call to the system
//! RNG on Windows (`RtlGenRandom`). No `rand`, `getrandom`, or `uuid` crate.
//!
//! # Panics
//!
//! ID generation panics if the OS CSPRNG cannot be read — an unrecoverable
//! environment failure, mirroring how the reference implementations treat it.

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The crate version, taken from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crockford base32 alphabet: the digits with `I`, `L`, `O`, and `U` removed.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

const MASK48: u128 = (1 << 48) - 1;

// ---------------------------------------------------------------------------
// OS CSPRNG — the only source of randomness, via std + a per-platform shim.
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn fill_random(buf: &mut [u8]) {
    use std::fs::File;
    use std::io::Read;
    use std::sync::OnceLock;
    // Open the kernel CSPRNG once and reuse the handle: re-opening per ID would
    // burn an fd + open/close syscall pair each call and can exhaust the process
    // fd limit under load. `&File: Read`, so concurrent reads need no &mut/lock
    // (each read syscall fills the small buffer atomically). Safe std I/O.
    static URANDOM: OnceLock<File> = OnceLock::new();
    let file = URANDOM.get_or_init(|| {
        File::open("/dev/urandom").expect("uid: cannot open /dev/urandom (OS CSPRNG unavailable)")
    });
    // `impl Read for &File`: read through a shared reference, no &mut / lock.
    let mut reader: &File = file;
    reader
        .read_exact(buf)
        .expect("uid: cannot read /dev/urandom (OS CSPRNG unavailable)");
}

// RtlGenRandom, exported from advapi32 as SystemFunction036 — the Windows system
// CSPRNG with no crate dependency (the same entry `getrandom` links). Returns
// nonzero on success. Declared at module scope (idiomatic) and gated to Windows.
#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    fn SystemFunction036(random_buffer: *mut u8, random_buffer_length: u32) -> u8;
}

#[cfg(windows)]
fn fill_random(buf: &mut [u8]) {
    // SAFETY: `buf` is a uniquely-owned (`&mut`) slice, so no other thread
    // aliases it during the call. We pass its pointer and exact byte length;
    // `buf.len() <= 16` here, so the `as u32` cannot truncate. RtlGenRandom
    // fills the entire range `[ptr, ptr + len)` on success; on failure (return
    // 0) we abort via the assert below rather than trust partial output.
    let ok = unsafe { SystemFunction036(buf.as_mut_ptr(), buf.len() as u32) };
    assert!(ok != 0, "uid: RtlGenRandom failed (OS CSPRNG unavailable)");
}

/// Read `nbytes` (<= 16) big-endian random bytes into a `u128`.
fn random_bits(nbytes: usize) -> u128 {
    debug_assert!(nbytes <= 16);
    let mut buf = [0u8; 16];
    fill_random(&mut buf[..nbytes]);
    // Only buf[..nbytes] is filled; the remaining bytes stay zero and unused.
    let mut value: u128 = 0;
    for &b in &buf[..nbytes] {
        value = (value << 8) | b as u128;
    }
    value
}

fn now_ms() -> u64 {
    // as_millis() is u128; u64::MAX ms is ~584 million years past the epoch, so
    // the narrowing cast is unreachable in any real system clock.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("uid: system clock is before the Unix epoch")
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Crockford base32 codec (128-bit values <-> 26 characters).
// ---------------------------------------------------------------------------

/// Encode a 128-bit value as 26 Crockford base32 characters (MSB first).
fn encode_crockford(mut value: u128) -> String {
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1f) as usize];
        value >>= 5;
    }
    // `out` is ASCII by construction.
    String::from_utf8(out.to_vec()).expect("Crockford output is ASCII")
}

/// Map one input character to its 5-bit value, accepting lowercase and the
/// Crockford aliases (`I`/`L` -> `1`, `O` -> `0`). `U` is not in the alphabet.
fn decode_digit(ch: char) -> Option<u8> {
    if !ch.is_ascii() {
        return None;
    }
    let up = match ch.to_ascii_uppercase() as u8 {
        b'I' | b'L' => b'1',
        b'O' => b'0',
        other => other,
    };
    CROCKFORD.iter().position(|&c| c == up).map(|p| p as u8)
}

/// Decode a 26-character Crockford base32 string to a 128-bit value.
fn decode_crockford(text: &str) -> Result<u128, UlidError> {
    // A valid ULID is 26 ASCII characters, so byte length == char count here;
    // any non-ASCII byte makes len != 26 or is rejected as an invalid char.
    if text.len() != 26 {
        return Err(UlidError::BadLength(text.len()));
    }
    let mut value: u128 = 0;
    for (i, ch) in text.chars().enumerate() {
        let digit = decode_digit(ch).ok_or(UlidError::InvalidChar(ch))?;
        // 26 * 5 = 130 bits: only the first character can push past 128 bits.
        // The leading 5-bit group sits at 2^125, so any value > 7 overflows.
        if i == 0 && digit > 7 {
            return Err(UlidError::Overflow);
        }
        value = (value << 5) | digit as u128;
    }
    Ok(value)
}

/// Error returned when parsing a ULID string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UlidError {
    /// The string was not exactly 26 characters.
    BadLength(usize),
    /// A character was not in the Crockford base32 alphabet.
    InvalidChar(char),
    /// The 26 characters encode a value larger than 128 bits.
    Overflow,
}

impl fmt::Display for UlidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UlidError::BadLength(n) => write!(f, "ULID must be 26 characters, got {n}"),
            UlidError::InvalidChar(c) => write!(f, "invalid ULID character: {c:?}"),
            UlidError::Overflow => write!(f, "ULID overflows 128 bits"),
        }
    }
}

impl std::error::Error for UlidError {}

// ---------------------------------------------------------------------------
// Monotonic-within-a-millisecond clock.
// ---------------------------------------------------------------------------

/// Thread-safe source of a per-millisecond monotonic random counter.
///
/// Holds the last timestamp and random value. Same millisecond -> increment the
/// counter; a clock that steps backwards still yields strictly increasing IDs.
/// `bits` is the width of the random field (80 for ULID, 74 for UUIDv7).
struct MonotonicClock {
    mask: u128,
    nbytes: usize,
    last_ms: i128, // -1 sentinel means "nothing seen yet"
    last_rand: u128,
}

impl MonotonicClock {
    const fn new(bits: u32) -> Self {
        // `1u128 << bits` requires bits < 128 (only 80 and 74 are used). This
        // assert is const-evaluated at every call site, so a bad width is a
        // compile error, not a runtime surprise.
        assert!(bits < 128, "MonotonicClock: bits must be < 128");
        MonotonicClock {
            mask: (1u128 << bits) - 1,
            nbytes: ((bits + 7) / 8) as usize,
            last_ms: -1,
            last_rand: 0,
        }
    }

    /// Return `(ms, rand)`. `ms = None` uses the wall clock and guarantees
    /// monotonicity even if it steps backwards; an explicit `ms` is honored
    /// verbatim (the caller owns ordering when they pin the timestamp).
    fn next(&mut self, ms: Option<u64>) -> (u64, u128) {
        let explicit = ms.is_some();
        let mut ms: i128 = ms.map_or_else(|| now_ms() as i128, |m| m as i128);
        let rand: u128;
        if ms > self.last_ms {
            self.last_ms = ms;
            rand = random_bits(self.nbytes) & self.mask;
        } else if ms == self.last_ms {
            // Same millisecond: increment the counter to preserve ordering.
            rand = self.last_rand + 1;
            assert!(
                rand <= self.mask,
                "uid: random component exhausted within one millisecond"
            );
        } else if explicit {
            // Caller pinned an earlier timestamp; honor it verbatim.
            self.last_ms = ms;
            rand = random_bits(self.nbytes) & self.mask;
        } else {
            // Wall clock stepped backwards (e.g. NTP): hold the last timestamp
            // and increment the counter so IDs never regress.
            ms = self.last_ms;
            rand = self.last_rand + 1;
            assert!(
                rand <= self.mask,
                "uid: random component exhausted within one millisecond"
            );
        }
        self.last_rand = rand;
        (ms as u64, rand)
    }
}

// Live clocks drive the default `ulid()` / `uuid7()`. The `_at` variants use
// SEPARATE clocks so pinning an earlier timestamp (backfills, tests) can never
// walk the live clock backwards and desequence subsequently issued IDs.
static ULID_CLOCK: Mutex<MonotonicClock> = Mutex::new(MonotonicClock::new(80));
static UUID7_CLOCK: Mutex<MonotonicClock> = Mutex::new(MonotonicClock::new(74));
static ULID_AT_CLOCK: Mutex<MonotonicClock> = Mutex::new(MonotonicClock::new(80));
static UUID7_AT_CLOCK: Mutex<MonotonicClock> = Mutex::new(MonotonicClock::new(74));

/// Lock a clock, recovering the guard even if a prior holder panicked (an
/// exhausted-counter panic leaves the clock state usable, so poisoning should
/// not permanently disable ID generation for the whole process).
fn lock(clock: &Mutex<MonotonicClock>) -> std::sync::MutexGuard<'_, MonotonicClock> {
    clock.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// UUID value type (std has none).
// ---------------------------------------------------------------------------

/// A 128-bit UUID. Ordering is by the raw big-endian bytes, so version-7
/// values sort by their embedded timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Wrap 16 raw bytes as a `Uuid`.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Uuid(bytes)
    }

    /// The raw 16 bytes, big-endian.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The 4-bit version field (7 for values from [`uuid7`]).
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// The embedded 48-bit millisecond Unix timestamp (the top 48 bits).
    pub fn timestamp_ms(&self) -> u64 {
        let mut ms: u64 = 0;
        for &b in &self.0[..6] {
            ms = (ms << 8) | b as u64;
        }
        ms
    }

    /// The embedded timestamp as a [`SystemTime`].
    pub fn system_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.timestamp_ms())
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }
}

// ---------------------------------------------------------------------------
// Public API.
// ---------------------------------------------------------------------------

/// Return a new ULID as a 26-character uppercase Crockford base32 string.
///
/// Layout: 48-bit millisecond timestamp + 80-bit monotonic randomness.
pub fn ulid() -> String {
    let (ts, rand) = lock(&ULID_CLOCK).next(None);
    encode_crockford(((ts as u128 & MASK48) << 80) | rand)
}

/// Like [`ulid`] but pins the timestamp to `ms` (mainly for tests / backfills).
///
/// Uses a clock separate from [`ulid`], so pinning a timestamp never affects the
/// ordering of IDs from [`ulid`].
pub fn ulid_at(ms: u64) -> String {
    let (ts, rand) = lock(&ULID_AT_CLOCK).next(Some(ms));
    encode_crockford(((ts as u128 & MASK48) << 80) | rand)
}

fn assemble_uuid7(ts: u64, rand: u128) -> Uuid {
    let rand_a = (rand >> 62) & 0xfff;
    let rand_b = rand & ((1u128 << 62) - 1);
    let mut value: u128 = (ts as u128 & MASK48) << 80;
    value |= 0x7u128 << 76; // version
    value |= rand_a << 64;
    value |= 0b10u128 << 62; // variant (RFC 4122)
    value |= rand_b;
    Uuid(value.to_be_bytes())
}

/// Return a new UUID version 7 (RFC 9562).
///
/// Layout: 48-bit millisecond timestamp, 4-bit version (7), 12-bit `rand_a`,
/// 2-bit variant (`0b10`), 62-bit `rand_b`. The 74 random bits act as a
/// monotonic counter within a millisecond.
pub fn uuid7() -> Uuid {
    let (ts, rand) = lock(&UUID7_CLOCK).next(None);
    assemble_uuid7(ts, rand)
}

/// Like [`uuid7`] but pins the timestamp to `ms` (mainly for tests / backfills).
///
/// Uses a clock separate from [`uuid7`], so pinning a timestamp never affects
/// the ordering of IDs from [`uuid7`].
pub fn uuid7_at(ms: u64) -> Uuid {
    let (ts, rand) = lock(&UUID7_AT_CLOCK).next(Some(ms));
    assemble_uuid7(ts, rand)
}

/// Return the 48-bit millisecond Unix timestamp embedded in a ULID string.
pub fn ulid_timestamp_ms(ulid: &str) -> Result<u64, UlidError> {
    Ok((decode_crockford(ulid)? >> 80) as u64)
}

/// Return the embedded timestamp of a ULID string as a [`SystemTime`].
pub fn ulid_system_time(ulid: &str) -> Result<SystemTime, UlidError> {
    Ok(UNIX_EPOCH + Duration::from_millis(ulid_timestamp_ms(ulid)?))
}

// ---------------------------------------------------------------------------
// White-box unit tests (access to the private codec / clock).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_zero_and_roundtrip() {
        assert_eq!(encode_crockford(0), "0".repeat(26));
        assert_eq!(decode_crockford(&"0".repeat(26)).unwrap(), 0);
        for value in [0u128, 1, 1 << 80, 1 << 127, u128::MAX] {
            assert_eq!(decode_crockford(&encode_crockford(value)).unwrap(), value);
        }
    }

    #[test]
    fn decode_rejects_bad_input() {
        assert_eq!(decode_crockford("too-short"), Err(UlidError::BadLength(9)));
        assert!(matches!(
            decode_crockford(&"U".repeat(26)),
            Err(UlidError::InvalidChar('U'))
        ));
        // Valid chars but > 128 bits (leading digit 8).
        assert_eq!(
            decode_crockford(&format!("8{}", "0".repeat(25))),
            Err(UlidError::Overflow)
        );
    }

    #[test]
    fn decode_accepts_lowercase_and_aliases() {
        let s = ulid_at(1_469_918_176_385);
        assert_eq!(
            decode_crockford(&s.to_lowercase()).unwrap(),
            decode_crockford(&s).unwrap()
        );
        assert_eq!(
            decode_crockford(&"O".repeat(26)),
            decode_crockford(&"0".repeat(26))
        );
        assert_eq!(
            decode_crockford(&"I".repeat(26)),
            decode_crockford(&"1".repeat(26))
        );
        assert_eq!(
            decode_crockford(&"L".repeat(26)),
            decode_crockford(&"1".repeat(26))
        );
    }

    #[test]
    fn ulid_monotonic_within_ms() {
        let mut clock = MonotonicClock::new(80);
        let mut prev: Option<String> = None;
        for _ in 0..1000 {
            let (_ts, rand) = clock.next(Some(42));
            let s = encode_crockford((42u128 << 80) | rand);
            if let Some(p) = &prev {
                assert!(*p < s, "ULIDs not strictly increasing within one ms");
            }
            prev = Some(s);
        }
    }

    #[test]
    fn uuid7_monotonic_within_ms() {
        // Property test on a fresh clock (the global clock is shared across the
        // parallel test threads, so pin it locally for determinism).
        let mut clock = MonotonicClock::new(74);
        let mut prev: Option<Uuid> = None;
        for _ in 0..1000 {
            let (ts, rand) = clock.next(Some(99));
            let u = assemble_uuid7(ts, rand);
            if let Some(p) = prev {
                assert!(p < u, "UUIDv7 values not strictly increasing within one ms");
            }
            prev = Some(u);
        }
    }

    #[test]
    fn wall_clock_backstop() {
        // Auto (None) path: a backwards wall clock must not regress the ts and
        // must increment the counter.
        let mut clock = MonotonicClock::new(80);
        clock.next(None); // seed with now()
        let future = clock.last_ms + 10_000;
        clock.last_ms = future;
        clock.last_rand = 5;
        let (ts, rand) = clock.next(None); // now() < future -> backstop
        assert_eq!(ts as i128, future, "timestamp must not regress");
        assert_eq!(rand, 6, "counter increments on regression");
    }

    #[test]
    fn explicit_ms_is_honored_verbatim() {
        assert_eq!(ulid_timestamp_ms(&ulid_at(1000)).unwrap(), 1000);
        assert_eq!(ulid_timestamp_ms(&ulid_at(500)).unwrap(), 500);
        assert_eq!(uuid7_at(500).timestamp_ms(), 500);
    }

    #[test]
    #[should_panic(expected = "exhausted within one millisecond")]
    fn counter_overflow_panics() {
        let mut clock = MonotonicClock::new(4); // tiny field forces overflow fast
        clock.next(Some(7));
        clock.last_rand = clock.mask; // saturate
        clock.next(Some(7));
    }

    #[test]
    #[should_panic(expected = "exhausted within one millisecond")]
    fn backstop_overflow_panics() {
        let mut clock = MonotonicClock::new(4);
        clock.next(None);
        clock.last_ms += 10_000; // pretend a later ms was already seen
        clock.last_rand = clock.mask; // saturate
        clock.next(None); // now() < last_ms -> backstop -> overflow
    }
}
