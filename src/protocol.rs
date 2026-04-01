pub const HEADER_LEN: usize = 8;

pub const DOMAIN_DB: u8 = 0x10;
pub const DOMAIN_NET: u8 = 0x20;
pub const DOMAIN_AUTH: u8 = 0x30;
pub const DOMAIN_EVENT: u8 = 0x80;

pub const DB_READ: u8 = 0x01;
pub const DB_WRITE: u8 = 0x02;
pub const DB_DELETE: u8 = 0x03;
pub const DB_DERIVE: u8 = 0x04;
pub const DB_MERGE: u8 = 0x05;
pub const DB_INSPECT: u8 = 0x06;

pub const NET_CONNECT: u8 = 0x21;
pub const NET_DIRECT_MSG: u8 = 0x22;
pub const NET_SUBSCRIBE: u8 = 0x23;
pub const NET_PUBLISH: u8 = 0x24;
pub const NET_ANNOUNCE: u8 = 0x25;
pub const NET_FIND: u8 = 0x26;

pub const AUTH_GEN_KEYPAIR: u8 = 0x31;
pub const AUTH_SIGN: u8 = 0x32;
pub const AUTH_VERIFY: u8 = 0x33;

pub const EVT_DIRECT_MSG: u8 = 0x01;
pub const EVT_GOSSIP_MSG: u8 = 0x02;
pub const EVT_PROVIDER_FOUND: u8 = 0x03;

pub const FLAG_FALLBACK_P2P: u8 = 0b0000_0001;
pub const FLAG_HAS_PASSPORT: u8 = 0b0000_0010;
pub const FLAG_ERROR: u8 = 0b1000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiarsabaHeader {
    pub domain: u8,
    pub flags: u8,
    pub req_id: u32,
    pub payload_len: u16,
}

pub fn parse_header(data: &[u8]) -> Result<DiarsabaHeader, &'static str> {
    if data.len() < HEADER_LEN {
        return Err("header requires at least 8 bytes");
    }

    let req_id = u32::from_le_bytes(
        data[2..6]
            .try_into()
            .map_err(|_| "invalid req_id bytes")?,
    );
    let payload_len = u16::from_be_bytes(
        data[6..8]
            .try_into()
            .map_err(|_| "invalid payload_len bytes")?,
    );

    Ok(DiarsabaHeader {
        domain: data[0],
        flags: data[1],
        req_id,
        payload_len,
    })
}

pub fn build_header(header: &DiarsabaHeader) -> [u8; HEADER_LEN] {
    let req_id = header.req_id.to_le_bytes();
    let payload_len = header.payload_len.to_be_bytes();

    [
        header.domain,
        header.flags,
        req_id[0],
        req_id[1],
        req_id[2],
        req_id[3],
        payload_len[0],
        payload_len[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_header_uses_mixed_endianness() {
        let header = DiarsabaHeader {
            domain: DOMAIN_NET,
            flags: FLAG_FALLBACK_P2P | FLAG_HAS_PASSPORT,
            req_id: 0x1234_5678,
            payload_len: 0x09ab,
        };

        let bytes = build_header(&header);

        assert_eq!(
            bytes,
            [
                DOMAIN_NET,
                FLAG_FALLBACK_P2P | FLAG_HAS_PASSPORT,
                0x78,
                0x56,
                0x34,
                0x12,
                0x09,
                0xab,
            ]
        );
    }

    #[test]
    fn parse_header_reads_mixed_endianness() {
        let bytes = [
            DOMAIN_AUTH,
            FLAG_HAS_PASSPORT,
            0xef,
            0xcd,
            0xab,
            0x90,
            0x12,
            0x34,
        ];

        let header = parse_header(&bytes).expect("header should parse");

        assert_eq!(
            header,
            DiarsabaHeader {
                domain: DOMAIN_AUTH,
                flags: FLAG_HAS_PASSPORT,
                req_id: 0x90ab_cdef,
                payload_len: 0x1234,
            }
        );
    }

    #[test]
    fn parse_header_rejects_short_input() {
        let err = parse_header(&[0; HEADER_LEN - 1]).expect_err("short header must fail");

        assert_eq!(err, "header requires at least 8 bytes");
    }

    #[test]
    fn parse_and_build_round_trip() {
        let original = DiarsabaHeader {
            domain: DOMAIN_DB,
            flags: FLAG_FALLBACK_P2P,
            req_id: 42,
            payload_len: 512,
        };

        let encoded = build_header(&original);
        let decoded = parse_header(&encoded).expect("round trip should parse");

        assert_eq!(decoded, original);
    }
}