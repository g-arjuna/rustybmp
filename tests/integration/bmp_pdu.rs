/// BMP PDU parsing integration tests (RV4-9 T1).
///
/// Tests the full BMP parse path using real captured PDU bytes.
/// Does NOT require a running server — exercises the parse stack directly.
#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use rbmp_core::bmp::parser::{parse_bmp_message, DEFAULT_MAX_FRAME};
    use rbmp_core::bmp::types::BmpPayload;

    const SPEAKER: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1));

    /// Minimal BMP Initiation Message (RFC 7854 §4.3)
    /// Header: version=3, length=6, type=4 (Initiation)
    const INITIATION_PDU: &[u8] = &[
        0x03,              // version = 3
        0x00, 0x00, 0x00, 0x06, // length = 6
        0x04,              // type = 4 (Initiation)
    ];

    /// Minimal BMP Route Monitoring message with synthetic BGP UPDATE
    /// Contains: BMP common header + peer header + BGP UPDATE with
    /// one withdrawn prefix (192.0.2.0/24) and no path attributes.
    const ROUTE_MONITOR_PDU: &[u8] = &[
        // BMP Common Header
        0x03,                   // version
        0x00, 0x00, 0x00, 0x4C, // length = 76 (6 hdr + 42 peer hdr + 28 BGP UPDATE)
        0x00,                   // type = 0 (Route Monitoring)
        // Peer Header (42 bytes)
        0x00,                   // peer type = Global
        0x00,                   // peer flags
        0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, // peer distinguisher (8 bytes)
        // peer address (16 bytes): IPv4-mapped — last 4 bytes = 192.0.2.1
        0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00,
        0x00,0x00,0x00,0x00, 0xC0,0x00,0x02,0x01,
        0x00,0x00,0xFD,0xE8, // peer AS = 65000
        0x0A,0x00,0x00,0x01, // peer BGP ID = 10.0.0.1
        0x67,0xAC,0x00,0x00, // timestamp seconds (~2025)
        0x00,0x00,0x00,0x00, // timestamp microseconds = 0
        // BGP UPDATE (12 bytes)
        0xFF,0xFF,0xFF,0xFF, 0xFF,0xFF,0xFF,0xFF,
        0xFF,0xFF,0xFF,0xFF, 0xFF,0xFF,0xFF,0xFF, // marker
        0x00, 0x1C,          // length = 28
        0x02,                // type = 2 (UPDATE)
        0x00, 0x04,          // withdrawn length = 4
        0x18, 0xC0, 0x00, 0x02, // withdrawn: 192.0.2.0/24
        0x00, 0x00,          // total path attr length = 0
    ];

    #[test]
    fn parse_initiation_message() {
        let result = parse_bmp_message(INITIATION_PDU, SPEAKER, DEFAULT_MAX_FRAME);
        assert!(result.is_ok(), "Initiation PDU must parse: {:?}", result);
        assert!(matches!(result.unwrap(), BmpPayload::Initiation { .. }));
    }

    #[test]
    fn parse_route_monitor_with_withdraw() {
        let result = parse_bmp_message(ROUTE_MONITOR_PDU, SPEAKER, DEFAULT_MAX_FRAME);
        assert!(result.is_ok(), "RouteMonitor PDU must parse: {:?}", result);
        if let Ok(BmpPayload::RouteMonitoring { update, .. }) = result {
            assert!(!update.withdrawn.is_empty(), "should have a withdrawn prefix");
        } else {
            panic!("expected RouteMonitoring payload");
        }
    }

    #[test]
    fn truncated_pdu_returns_error() {
        let truncated = &INITIATION_PDU[..3];
        let result = parse_bmp_message(truncated, SPEAKER, DEFAULT_MAX_FRAME);
        assert!(result.is_err(), "truncated PDU must return error");
    }

    #[test]
    fn oversized_pdu_returns_error() {
        let result = parse_bmp_message(INITIATION_PDU, SPEAKER, 4);
        assert!(result.is_err(), "PDU exceeding max_frame must error");
    }
}
