/// Integration test harness for rustybmp (RV4-9).
///
/// bmp_pdu: pure parse-layer tests using rbmp_core only (no server needed).
/// api_smoke / retention_sweep: require rbmp-server as a library target;
/// run manually against a live instance via docs/UBUNTU_TESTING.md.
///
/// Run with:
///   cargo test --test integration -- --nocapture
pub mod bmp_pdu;
pub mod frr_bmp;
