//! CRC16-CCITT-FALSE — the frame integrity check.
//!
//! Per `control-protocol.md` §2, the CRC covers `TYPE | SEQ | LEN | PAYLOAD` (not SOF, not the CRC
//! bytes). Port of `medius.py`'s `crc16_ccitt` / firmware's `crc16_ccitt_update`.

/// Compute the CRC16-CCITT-FALSE of `data`: poly `0x1021`, init `0xFFFF`, MSB-first, no reflection,
/// no final XOR.
///
/// # Examples
/// ```ignore
/// # use medius::protocol::crc::crc16_ccitt;
/// assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
/// assert_eq!(crc16_ccitt(b""), 0xFFFF);
/// ```
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}
