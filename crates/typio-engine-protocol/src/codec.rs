use crate::{ProtocolError, Result};

/// Encode arbitrary bytes as lowercase hexadecimal text.
pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Encode UTF-8 text as lowercase hexadecimal bytes.
pub fn encode_hex_string(value: &str) -> String {
    encode_hex(value.as_bytes())
}

/// Decode hexadecimal text into arbitrary bytes.
pub fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(ProtocolError::invalid("odd-length hexadecimal payload"));
    }

    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for index in (0..bytes.len()).step_by(2) {
        output.push((hex_nibble(bytes[index])? << 4) | hex_nibble(bytes[index + 1])?);
    }
    Ok(output)
}

/// Decode hexadecimal UTF-8 text.
pub fn decode_hex_string(value: &str) -> Result<String> {
    String::from_utf8(decode_hex(value)?)
        .map_err(|error| ProtocolError::invalid(format!("invalid UTF-8 payload: {error}")))
}

fn hex_nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(ProtocolError::invalid(format!(
            "invalid hexadecimal digit 0x{value:02x}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexadecimal_round_trip_preserves_unicode_and_binary() {
        assert_eq!(
            decode_hex_string(&encode_hex_string("输入法")).unwrap(),
            "输入法"
        );
        assert_eq!(
            decode_hex(&encode_hex(&[0, 1, 0xfe, 0xff])).unwrap(),
            [0, 1, 0xfe, 0xff]
        );
    }

    #[test]
    fn hexadecimal_decoder_rejects_invalid_input() {
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("xx").is_err());
        assert!(decode_hex_string("ff").is_err());
    }
}
