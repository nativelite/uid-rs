//! Spec-anchored black-box tests for the public `uid` API.
//!
//! Correctness is checked against the canonical formats: the ULID Crockford
//! base32 layout and the UUIDv7 bit layout from RFC 9562, plus the
//! monotonic-within-a-millisecond property and the CLI contract.

use std::process::Command;

const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn is_crockford(s: &str) -> bool {
    s.chars().all(|c| CROCKFORD.contains(c))
}

#[test]
fn ulid_length_and_alphabet() {
    let s = uid::ulid();
    assert_eq!(s.len(), 26);
    assert!(is_crockford(&s), "non-Crockford chars in {s:?}");
}

#[test]
fn ulid_first_char_fits_128_bits() {
    // 26 * 5 = 130 bits encode a 128-bit value, so the top 2 bits are 0 and the
    // first character can never exceed '7'.
    for _ in 0..200 {
        assert!(uid::ulid().as_bytes()[0] <= b'7');
    }
}

#[test]
fn ulid_known_timestamp_vector() {
    // Canonical ULID spec example: 1469918176385 ms -> "01ARYZ6S41".
    let s = uid::ulid_at(1_469_918_176_385);
    assert_eq!(&s[..10], "01ARYZ6S41");
    assert_eq!(uid::ulid_timestamp_ms(&s).unwrap(), 1_469_918_176_385);
}

#[test]
fn uuid7_version_and_variant() {
    let u = uid::uuid7();
    assert_eq!(u.version(), 7);
    // RFC 4122 variant: the two most significant bits of byte 8 are 0b10.
    assert_eq!(u.as_bytes()[8] >> 6, 0b10);
}

#[test]
fn uuid7_timestamp_embedded() {
    let u = uid::uuid7_at(1_469_918_176_385);
    assert_eq!(u.timestamp_ms(), 1_469_918_176_385);
}

#[test]
fn uuid7_display_shape() {
    let text = uid::uuid7().to_string();
    assert_eq!(text.len(), 36);
    let groups: Vec<&str> = text.split('-').collect();
    assert_eq!(
        groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12]
    );
    assert!(text.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    // Version nibble is the 15th hex char (start of the 3rd group).
    assert_eq!(groups[2].as_bytes()[0], b'7');
}

#[test]
fn ids_are_distinct() {
    assert_ne!(uid::ulid(), uid::ulid());
    assert_ne!(uid::uuid7(), uid::uuid7());
}

#[test]
fn live_ids_sort_by_creation_order() {
    // The core guarantee: successive live IDs strictly increase (same ms ->
    // counter increment; later ms -> larger timestamp). Holds even if other
    // threads interleave, since the shared clock never regresses.
    let mut prev = uid::ulid();
    for _ in 0..1000 {
        let next = uid::ulid();
        assert!(prev < next, "live ULIDs must sort by creation order");
        prev = next;
    }
}

#[test]
fn ulid_and_uuid7_agree_on_time() {
    let ms = 1_700_000_000_000;
    assert_eq!(
        uid::ulid_timestamp_ms(&uid::ulid_at(ms)).unwrap(),
        uid::uuid7_at(ms).timestamp_ms()
    );
}

#[test]
fn timestamp_roundtrips_through_system_time() {
    use std::time::{Duration, UNIX_EPOCH};
    let ms = 1_469_918_176_385u64;
    assert_eq!(
        uid::uuid7_at(ms).system_time(),
        UNIX_EPOCH + Duration::from_millis(ms)
    );
    assert_eq!(
        uid::ulid_system_time(&uid::ulid_at(ms)).unwrap(),
        UNIX_EPOCH + Duration::from_millis(ms)
    );
}

#[test]
fn ulid_timestamp_ms_rejects_bad_input() {
    assert!(uid::ulid_timestamp_ms("too-short").is_err());
    assert!(uid::ulid_timestamp_ms(&"U".repeat(26)).is_err());
}

// --- CLI contract: run the actual built binary. -------------------------------

fn run_cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_uid"))
        .args(args)
        .output()
        .expect("failed to run the uid binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap().trim().to_string(),
    )
}

#[test]
fn cli_default_prints_ulid() {
    let (code, out) = run_cli(&[]);
    assert_eq!(code, 0);
    assert_eq!(out.len(), 26);
    assert!(is_crockford(&out));
}

#[test]
fn cli_uuid7_subcommand() {
    let (code, out) = run_cli(&["uuid7"]);
    assert_eq!(code, 0);
    assert_eq!(out.len(), 36);
    assert_eq!(out.split('-').nth(2).unwrap().as_bytes()[0], b'7');
}

#[test]
fn cli_unknown_kind_errors() {
    let (code, _) = run_cli(&["nope"]);
    assert_eq!(code, 2);
}
