const FEND: u8 = 0xC0;
const FESC: u8 = 0xDB;
const TFEND: u8 = 0xDC;
const TFESC: u8 = 0xDD;

pub fn encode_data_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 3);
    frame.push(FEND);
    frame.push(0x00);

    for &byte in payload {
        match byte {
            FEND => {
                frame.push(FESC);
                frame.push(TFEND);
            }
            FESC => {
                frame.push(FESC);
                frame.push(TFESC);
            }
            _ => frame.push(byte),
        }
    }

    frame.push(FEND);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_data_frame_with_escaping() {
        let encoded = encode_data_frame(&[b'A', FEND, FESC, b'Z']);
        assert_eq!(
            encoded,
            vec![FEND, 0x00, b'A', FESC, TFEND, FESC, TFESC, b'Z', FEND]
        );
    }
}
