//! CRC16-CCITT-FALSE — the frame integrity check.
//!
//! Per `docs/protocol/control-protocol.md` §2, every frame carries a `CRC16` computed over
//! `TYPE | SEQ | LEN | PAYLOAD` (not the SOF byte, not the CRC bytes themselves). The algorithm is
//! CCITT-FALSE: polynomial `0x1021`, initial value `0xFFFF`, MSB-first, no input/output reflection,
//! no final XOR. This is a faithful port of `tools/medius.py`'s `crc16_ccitt` and the firmware's
//! `crc16_ccitt_update`.

/// Compute the CRC16-CCITT-FALSE of `data`.
///
/// - Polynomial `0x1021`, initial value `0xFFFF`, MSB-first, no reflection, no final XOR.
/// - Pure and panic-free for any input.
///
/// # Examples
/// ```
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical CCITT-FALSE check value for the ASCII string "123456789".
    #[test]
    fn crc_standard_vector() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    /// Empty input returns the initial value untouched.
    #[test]
    fn crc_empty() {
        assert_eq!(crc16_ccitt(b""), 0xFFFF);
    }
}
