/// AGW (Packet Radio Application Gateway Interface) binary protocol handler.
/// AGW frames have a 36-byte header followed by variable-length data.
///
/// Binary frame format from actual Dire Wolf server.c agwpe_s struct:
/// - [0]      portx: u8 (radio port, 0-based)
/// - [1..3]   reserved: u8 x3
/// - [4]      datakind: u8 (command/type letter)
/// - [5]      reserved: u8
/// - [6]      pid: u8
/// - [7]      reserved: u8
/// - [8..17]  call_from: [u8; 10] (null-padded ASCII)
/// - [18..27] call_to: [u8; 10] (null-padded ASCII)
/// - [28..31] data_len: u32 (little-endian, _NETLE)
/// - [32..35] user_reserved: u32 (little-endian)
pub const AGW_HEADER_LEN: usize = 36;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgwFrame {
    pub port: u8,
    pub kind: u8,
    pub from_call: String,
    pub to_call: String,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct AgwBinaryDecoder {
    buffer: Vec<u8>,
}

impl AgwBinaryDecoder {
    /// Ingest bytes and return any complete frames.
    pub fn ingest(&mut self, bytes: &[u8]) -> Vec<AgwFrame> {
        let mut frames = Vec::new();

        self.buffer.extend_from_slice(bytes);

        loop {
            if self.buffer.len() < AGW_HEADER_LEN {
                break;
            }

            let data_len_bytes = [
                self.buffer[28],
                self.buffer[29],
                self.buffer[30],
                self.buffer[31],
            ];
            // AGWPE defines data_len as little-endian, and Dire Wolf sends
            // it that way on every host (its host2netle macro only swaps on
            // big-endian machines). The big-endian fallback is a guard
            // against a nonconforming server.
            const MAX_AGW_DATA_LEN: usize = 65_536;
            let data_len = {
                let le = u32::from_le_bytes(data_len_bytes) as usize;
                if le <= MAX_AGW_DATA_LEN {
                    le
                } else {
                    u32::from_be_bytes(data_len_bytes) as usize
                }
            };

            // Reject implausibly large frames to avoid stalling on malformed data.
            if data_len > MAX_AGW_DATA_LEN {
                // Skip one byte and attempt to resync.
                self.buffer.drain(..1);
                continue;
            }

            let frame_size = AGW_HEADER_LEN + data_len;
            if self.buffer.len() < frame_size {
                break;
            }

            if let Some(frame) = parse_agw_frame(&self.buffer[..frame_size]) {
                frames.push(frame);
                self.buffer.drain(..frame_size);
            } else {
                // Skip bad frame
                self.buffer.drain(..1);
            }
        }

        frames
    }
}

const fn make_startup_frame(kind: u8) -> [u8; AGW_HEADER_LEN] {
    let mut f = [0u8; AGW_HEADER_LEN];
    f[4] = kind;
    f
}

const R_FRAME: [u8; AGW_HEADER_LEN] = make_startup_frame(b'R');
const G_FRAME: [u8; AGW_HEADER_LEN] = make_startup_frame(b'G');
const X_FRAME: [u8; AGW_HEADER_LEN] = make_startup_frame(b'X');
const K_FRAME: [u8; AGW_HEADER_LEN] = make_startup_frame(b'k');
const M_FRAME: [u8; AGW_HEADER_LEN] = make_startup_frame(b'm');

const STARTUP_FRAMES: &[&[u8]] = &[&R_FRAME, &G_FRAME, &X_FRAME, &K_FRAME, &M_FRAME];

pub fn build_startup_command_frames() -> &'static [&'static [u8]] {
    STARTUP_FRAMES
}

pub fn parse_agw_frame(frame_bytes: &[u8]) -> Option<AgwFrame> {
    if frame_bytes.len() < AGW_HEADER_LEN {
        return None;
    }

    let port = frame_bytes[0];
    let kind = frame_bytes[4]; // DataKind is at byte 4

    let from_call = extract_call(&frame_bytes[8..18]);   // call_from at [8..17]
    let to_call = extract_call(&frame_bytes[18..28]);    // call_to at [18..27]

    let data_len_bytes = [
        frame_bytes[28],
        frame_bytes[29],
        frame_bytes[30],
        frame_bytes[31],
    ];
    // AGWPE defines data_len as little-endian; see AgwBinaryDecoder::ingest.
    const MAX_AGW_DATA_LEN: usize = 65_536;
    let data_len = {
        let le = u32::from_le_bytes(data_len_bytes) as usize;
        if le <= MAX_AGW_DATA_LEN {
            le
        } else {
            u32::from_be_bytes(data_len_bytes) as usize
        }
    };

    if frame_bytes.len() < AGW_HEADER_LEN + data_len {
        return None;
    }

    let data = frame_bytes[AGW_HEADER_LEN..AGW_HEADER_LEN + data_len].to_vec();

    Some(AgwFrame {
        port,
        kind,
        from_call,
        to_call,
        data,
    })
}

fn extract_call(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agw_frame_basic() {
        let mut frame = vec![0u8; AGW_HEADER_LEN];
        frame[0] = 0;      // portx
        frame[4] = b'K';   // DataKind at byte 4
        frame[6] = 0xF0;   // pid at byte 6

        // call_from at [8..17]
        let from = b"W1AW";
        frame[8..8 + from.len()].copy_from_slice(from);

        // call_to at [18..27]
        let to = b"APWOLF";
        frame[18..18 + to.len()].copy_from_slice(to);

        // data_len = 5 at bytes [28..32]
        frame[28..32].copy_from_slice(&5u32.to_le_bytes());

        frame.extend_from_slice(b"Hello");

        let parsed = parse_agw_frame(&frame).unwrap();
        assert_eq!(parsed.kind, b'K');
        assert_eq!(parsed.from_call, "W1AW");
        assert_eq!(parsed.to_call, "APWOLF");
        assert_eq!(parsed.data, b"Hello");
    }

    #[test]
    fn decoder_accumulates_then_parses() {
        let mut decoder = AgwBinaryDecoder::default();

        let mut frame = vec![0u8; AGW_HEADER_LEN];
        frame[4] = b'K'; // DataKind at byte 4
        // call_from at [8..17]
        frame[8] = b'N';
        frame[9] = b'0';
        // call_to at [18..27]
        frame[18] = b'T';
        frame[19] = b'E';
        frame[28..32].copy_from_slice(&3u32.to_le_bytes());
        frame.extend_from_slice(b"ABC");

        let frames = decoder.ingest(&frame[..20]);
        assert!(frames.is_empty());

        let frames = decoder.ingest(&frame[20..]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].to_call, "TE");
    }

    #[test]
    fn startup_command_sequence_is_expected() {
        let frames = build_startup_command_frames();
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0][4], b'R');
        assert_eq!(frames[1][4], b'G');
        assert_eq!(frames[2][4], b'X');
        assert_eq!(frames[3][4], b'k');
        assert_eq!(frames[4][4], b'm');
        assert!(frames.iter().all(|f| f.len() == AGW_HEADER_LEN));
    }
}
