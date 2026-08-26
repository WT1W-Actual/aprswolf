#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedAddress {
    pub callsign: String,
    pub ssid: u8,
    pub repeated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUiFrame {
    pub destination: ParsedAddress,
    pub source: ParsedAddress,
    pub digipeaters: Vec<ParsedAddress>,
    pub control: u8,
    pub pid: u8,
    pub info: Vec<u8>,
}

/// Encode a callsign+SSID into the 7-byte AX.25 address field.
/// Each character is shifted left by 1 bit; the SSID byte carries the SSID
/// in bits 4-1 and a reserved/H bit in bit 5.
/// `last` sets bit 0 of the final byte to mark the end of the address field.
fn encode_address(callsign: &str, ssid: u8, last: bool, has_been_repeated: bool) -> [u8; 7] {
    let mut buf = [b' ' << 1; 7];

    for (i, ch) in callsign.chars().take(6).enumerate() {
        buf[i] = (ch.to_ascii_uppercase() as u8) << 1;
    }

    // SSID byte: bit 7 = 1 (reserved), bit 6 = H (has-been-repeated), bits 4-1 = ssid
    let h_bit: u8 = if has_been_repeated { 0x80 } else { 0x00 };
    buf[6] = 0x60 | h_bit | ((ssid & 0x0F) << 1) | (if last { 0x01 } else { 0x00 });
    buf
}

fn decode_address(bytes: &[u8]) -> Result<(ParsedAddress, bool), String> {
    if bytes.len() != 7 {
        return Err("AX.25 address must be 7 bytes".to_string());
    }

    let callsign = bytes[..6]
        .iter()
        .map(|byte| (byte >> 1) as char)
        .collect::<String>()
        .trim_end()
        .to_string();
    let ssid = (bytes[6] >> 1) & 0x0F;
    let repeated = bytes[6] & 0x80 != 0;
    let last = bytes[6] & 0x01 != 0;

    Ok((
        ParsedAddress {
            callsign,
            ssid,
            repeated,
        },
        last,
    ))
}

/// Build a minimal AX.25 UI frame ready to hand to `encode_data_frame`.
///
/// Produces: dest_addr(7) + src_addr(7) + digi_addrs(7*n) + ctrl(1) + pid(1) + info(n)
pub fn build_ui_frame(
    destination: &str,
    source: &str,
    ssid: u8,
    digipeaters: &[&str],
    info: &[u8],
) -> Vec<u8> {
    let mut frame: Vec<u8> = Vec::new();

    // Destination address: not last in address field
    let (dest_call, dest_ssid) = split_callsign_ssid(destination);
    frame.extend_from_slice(&encode_address(&dest_call, dest_ssid, false, false));

    // Source address: last ONLY if no digipeaters
    let src_last = digipeaters.is_empty();
    frame.extend_from_slice(&encode_address(source, ssid, src_last, false));

    // Digipeater addresses
    for (i, digi) in digipeaters.iter().enumerate() {
        let (digi_call, digi_ssid) = split_callsign_ssid(digi);
        let is_last = i == digipeaters.len() - 1;
        frame.extend_from_slice(&encode_address(&digi_call, digi_ssid, is_last, false));
    }

    // Control: UI frame = 0x03
    frame.push(0x03);
    // PID: No layer 3 protocol = 0xF0
    frame.push(0xF0);

    // Info field
    frame.extend_from_slice(info);

    frame
}

/// Build an APRS uncompressed position info field.
///
/// Format: `!DDMM.mmN/DDDMM.mmW[symbol_code][comment]`
///
/// `symbol_table` is `/` (primary) or `\` (alternate).
/// `symbol_code` is a single ASCII character (e.g. `-` for house, `>` for car).
///
/// Minutes are rounded to centi-minutes with carry into the degrees field, so
/// the minutes field can never read 60.00 (invalid per the APRS spec) for
/// inputs like 42.999999°.
pub fn build_aprs_position_info(
    lat_deg: f64,
    lon_deg: f64,
    symbol_table: char,
    symbol_code: char,
    comment: &str,
) -> Vec<u8> {
    /// Convert decimal degrees to (degrees, centi-minutes), carrying when
    /// rounding pushes minutes to 60.00. Clamps at `max_deg`.
    fn deg_centimin(abs: f64, max_deg: u32) -> (u32, u32) {
        let total_cm = (abs * 6000.0).round() as u64;
        let mut d = (total_cm / 6000) as u32;
        let mut cm = (total_cm % 6000) as u32;
        if d > max_deg {
            d = max_deg;
            cm = 0;
        }
        (d, cm)
    }

    let (lat_d, lat_cm) = deg_centimin(lat_deg.abs(), 90);
    let lat_hemi = if lat_deg >= 0.0 { 'N' } else { 'S' };

    let (lon_d, lon_cm) = deg_centimin(lon_deg.abs(), 180);
    let lon_hemi = if lon_deg >= 0.0 { 'E' } else { 'W' };

    format!(
        "!{:02}{:02}.{:02}{}{}{:03}{:02}.{:02}{}{}{}",
        lat_d,
        lat_cm / 100,
        lat_cm % 100,
        lat_hemi,
        symbol_table,
        lon_d,
        lon_cm / 100,
        lon_cm % 100,
        lon_hemi,
        symbol_code,
        comment
    )
    .into_bytes()
}

/// Validate an APRS message id per spec: 1-5 alphanumeric characters,
/// optionally in reply-ack form `MM}` or `MM}AA` (each part 1-5 alnum,
/// the reply-ack part may be empty).
fn is_valid_msgid(id: &str) -> bool {
    let part_ok = |part: &str, allow_empty: bool| {
        (allow_empty || !part.is_empty())
            && part.len() <= 5
            && part.chars().all(|c| c.is_ascii_alphanumeric())
    };
    match id.split_once('}') {
        Some((msg, reply)) => part_ok(msg, false) && part_ok(reply, true),
        None => part_ok(id, false),
    }
}

pub fn build_aprs_message_info(addressee: &str, text: &str, message_id: Option<&str>) -> Vec<u8> {
    let clean_addressee = addressee.trim().to_uppercase();
    // APRS spec: addressee field must be exactly 9 characters. Truncate if over-length.
    let capped = &clean_addressee[..clean_addressee.len().min(9)];
    let padded = format!("{capped:<9}");
    match message_id {
        Some(id) if is_valid_msgid(id) => format!(":{padded}:{text}{{{id}").into_bytes(),
        _ => format!(":{padded}:{text}").into_bytes(),
    }
}

pub fn parse_ui_frame(frame: &[u8]) -> Result<ParsedUiFrame, String> {
    if frame.len() < 16 {
        return Err("AX.25 frame too short".to_string());
    }

    let (destination, _) = decode_address(&frame[0..7])?;
    let (source, mut last) = decode_address(&frame[7..14])?;
    let mut cursor = 14;
    let mut digipeaters = Vec::new();

    while !last {
        if frame.len() < cursor + 7 {
            return Err("AX.25 frame ended mid-address".to_string());
        }
        let (digi, is_last) = decode_address(&frame[cursor..cursor + 7])?;
        digipeaters.push(digi);
        last = is_last;
        cursor += 7;
    }

    if frame.len() < cursor + 2 {
        return Err("AX.25 frame missing control/PID".to_string());
    }

    Ok(ParsedUiFrame {
        destination,
        source,
        digipeaters,
        control: frame[cursor],
        pid: frame[cursor + 1],
        info: frame[cursor + 2..].to_vec(),
    })
}

/// Parsed APRS message information field.
pub struct ParsedAprsMessage {
    pub addressee: String,
    pub text: String,
    pub msgid: Option<String>,
}

/// Parse an APRS message info field into its components.
///
/// APRS message format: `:{ADDRESSEE:<9}:{text}` or `:{ADDRESSEE:<9}:{text}{msgid}`
///
/// Returns `None` if the field is not a well-formed APRS message.
pub fn parse_aprs_message(info_text: &str) -> Option<ParsedAprsMessage> {
    // Must start with ':' and have the second ':' at position 10 (9-char addressee field)
    let inner = info_text.strip_prefix(':')?;
    if inner.len() < 10 || inner.as_bytes()[9] != b':' {
        return None;
    }
    let addressee = inner[..9].trim().to_uppercase();
    let body = &inner[10..];
    if let Some(brace) = body.rfind('{') {
        let text = body[..brace].to_string();
        let msgid = body[brace + 1..].trim_end().to_string();
        Some(ParsedAprsMessage {
            addressee,
            text,
            msgid: Some(msgid),
        })
    } else {
        Some(ParsedAprsMessage {
            addressee,
            text: body.to_string(),
            msgid: None,
        })
    }
}

fn mic_e_lat_digit(ch: char) -> Option<u8> {
    match ch {
        '0'..='9' => Some((ch as u8) - b'0'),
        'A'..='J' => Some((ch as u8) - b'A'),
        'P'..='Y' => Some((ch as u8) - b'P'),
        // Position ambiguity placeholders. Treat as zero but mark ambiguous.
        'K' | 'L' | 'Z' => Some(0),
        _ => None,
    }
}

fn mic_e_is_west(ch: char) -> bool {
    matches!(ch, 'P'..='Z')
}

fn mic_e_has_lon_offset(ch: char) -> bool {
    matches!(ch, 'P'..='Z')
}

fn mic_e_is_north(ch: char) -> bool {
    matches!(ch, 'P'..='Z')
}

enum MicECommentBitKind {
    Zero,
    Standard,
    Custom,
}

fn mic_e_comment_bit_kind(ch: char) -> Option<MicECommentBitKind> {
    match ch {
        '0'..='9' | 'L' => Some(MicECommentBitKind::Zero),
        'A'..='K' => Some(MicECommentBitKind::Custom),
        'P'..='Z' => Some(MicECommentBitKind::Standard),
        _ => None,
    }
}

fn decode_mic_e_position_comment(dst_chars: &[char]) -> Option<&'static str> {
    if dst_chars.len() < 3 {
        return None;
    }

    let kinds: Vec<MicECommentBitKind> = dst_chars[..3]
        .iter()
        .copied()
        .map(mic_e_comment_bit_kind)
        .collect::<Option<Vec<_>>>()?;

    let has_standard = kinds.iter().any(|kind| matches!(kind, MicECommentBitKind::Standard));
    let has_custom = kinds.iter().any(|kind| matches!(kind, MicECommentBitKind::Custom));
    if has_standard && has_custom {
        return Some("comment=unknown");
    }

    let bits: Vec<bool> = kinds
        .iter()
        .map(|kind| !matches!(kind, MicECommentBitKind::Zero))
        .collect();

    let a = bits[0];
    let b = bits[1];
    let c = bits[2];

    if !a && !b && !c {
        return Some("comment=Emergency");
    }

    let index = match (a, b, c) {
        (true, true, true) => 0,
        (true, true, false) => 1,
        (true, false, true) => 2,
        (true, false, false) => 3,
        (false, true, true) => 4,
        (false, true, false) => 5,
        (false, false, true) => 6,
        (false, false, false) => unreachable!(),
    };

    const STANDARD: [&str; 7] = [
        "comment=M0 Off Duty",
        "comment=M1 En Route",
        "comment=M2 In Service",
        "comment=M3 Returning",
        "comment=M4 Committed",
        "comment=M5 Special",
        "comment=M6 Priority",
    ];
    const CUSTOM: [&str; 7] = [
        "comment=C0 Custom-0",
        "comment=C1 Custom-1",
        "comment=C2 Custom-2",
        "comment=C3 Custom-3",
        "comment=C4 Custom-4",
        "comment=C5 Custom-5",
        "comment=C6 Custom-6",
    ];

    if has_standard {
        Some(STANDARD[index])
    } else {
        Some(CUSTOM[index])
    }
}

fn format_mic_e_comment(comment: &[u8]) -> String {
    let mut out = String::with_capacity(comment.len());
    for &byte in comment {
        if byte.is_ascii_graphic() || byte == b' ' {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("<0x{byte:02X}>"));
        }
    }
    out.trim_matches('\0').trim().to_string()
}

fn is_maidenhead_locator(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes.len() {
        4 => {
            bytes[0].is_ascii_uppercase()
                && bytes[1].is_ascii_uppercase()
                && bytes[2].is_ascii_digit()
                && bytes[3].is_ascii_digit()
        }
        6 => {
            bytes[0].is_ascii_uppercase()
                && bytes[1].is_ascii_uppercase()
                && bytes[2].is_ascii_digit()
                && bytes[3].is_ascii_digit()
                && bytes[4].is_ascii_uppercase()
                && bytes[5].is_ascii_uppercase()
        }
        _ => false,
    }
}

fn extract_mic_e_grid_locator(comment: &str) -> Option<String> {
    for token in comment.split_whitespace() {
        let candidate = token
            .strip_prefix('>')
            .or_else(|| token.strip_prefix(']'))
            .unwrap_or(token);
        let locator = candidate.strip_suffix("/G").unwrap_or(candidate);
        if is_maidenhead_locator(locator) {
            return Some(locator.to_string());
        }
    }
    None
}

fn extract_mic_e_altitude_feet(comment: &str) -> Option<i32> {
    let marker = "/A=";
    let start = comment.find(marker)? + marker.len();
    let rest = &comment[start..];

    let candidate = if let Some(negative) = rest.strip_prefix('-') {
        let digits: String = negative.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.len() == 5 {
            format!("-{digits}")
        } else {
            return None;
        }
    } else {
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.len() == 6 {
            digits
        } else {
            return None;
        }
    };

    candidate.parse::<i32>().ok()
}

/// Decode a Mic-E information field into a compact human-readable summary.
///
/// Returns `None` if the frame is not Mic-E or cannot be decoded.
/// Decode a Mic-E packet given the destination callsign string (SSID already
/// stripped) and raw info bytes.  This is the shared implementation used by
/// both the AX.25 TNC path (`decode_mic_e`) and the APRS-IS TNC2 path.
pub fn decode_mic_e_from_parts(dst_callsign: &str, info: &[u8]) -> Option<String> {
    if info.len() < 9 {
        return None;
    }

    let info_type = info[0];
    if info_type != b'`' && info_type != b'\'' {
        return None;
    }

    let mut dst = dst_callsign.to_string();
    while dst.len() < 6 {
        dst.push(' ');
    }
    let dst_chars: Vec<char> = dst.chars().take(6).collect();
    if dst_chars.len() < 6 {
        return None;
    }

    let d0 = mic_e_lat_digit(dst_chars[0])?;
    let d1 = mic_e_lat_digit(dst_chars[1])?;
    let d2 = mic_e_lat_digit(dst_chars[2])?;
    let d3 = mic_e_lat_digit(dst_chars[3])?;
    let d4 = mic_e_lat_digit(dst_chars[4])?;
    let d5 = mic_e_lat_digit(dst_chars[5])?;

    let mut ambiguity = false;
    for ch in &dst_chars {
        if matches!(ch, 'K' | 'L' | 'Z') {
            ambiguity = true;
        }
    }

    let lat_deg = (d0 as f64) * 10.0 + (d1 as f64);
    let lat_min = (d2 as f64) * 10.0 + (d3 as f64) + (((d4 as f64) * 10.0 + (d5 as f64)) / 100.0);
    let mut lat = lat_deg + (lat_min / 60.0);
    if !mic_e_is_north(dst_chars[3]) {
        lat = -lat;
    }

    let mut lon_deg = (info[1] as i32) - 28;
    if mic_e_has_lon_offset(dst_chars[4]) {
        lon_deg += 100;
    }
    if (180..=189).contains(&lon_deg) {
        lon_deg -= 80;
    } else if (190..=199).contains(&lon_deg) {
        lon_deg -= 190;
    }
    if !(0..=180).contains(&lon_deg) {
        return None;
    }

    let mut lon_min = (info[2] as i32) - 28;
    if lon_min >= 60 {
        lon_min -= 60;
    }
    let mut lon_hun = (info[3] as i32) - 28;
    if lon_hun >= 60 {
        lon_hun -= 60;
    }
    if lon_min < 0 || lon_hun < 0 {
        return None;
    }

    let mut lon = (lon_deg as f64) + (((lon_min as f64) + ((lon_hun as f64) / 100.0)) / 60.0);
    if mic_e_is_west(dst_chars[5]) {
        lon = -lon;
    }

    let mut speed = ((info[4] as i32) - 28) * 10 + (((info[5] as i32) - 28) / 10);
    if speed >= 800 {
        speed -= 800;
    }
    if speed < 0 {
        speed = 0;
    }

    let mut course = ((((info[5] as i32) - 28) % 10) * 100) + ((info[6] as i32) - 28);
    if course >= 400 {
        course -= 400;
    }
    if course < 0 {
        course = 0;
    }

    let symbol_code = info[7] as char;
    let symbol_table = info[8] as char;
    let capability = if info_type == b'`' {
        "msg-capable"
    } else {
        "tracker"
    };

    let comment = if info.len() > 9 {
        format_mic_e_comment(&info[9..])
    } else {
        String::new()
    };

    let mut summary = format!(
        "Mic-E pos {:.5},{:.5} spd={}kt crs={} sym={}{} ({})",
        lat,
        lon,
        speed,
        course,
        symbol_table,
        symbol_code,
        capability
    );
    if let Some(comment_type) = decode_mic_e_position_comment(&dst_chars) {
        summary.push_str(&format!(" {}", comment_type));
    }
    if ambiguity {
        summary.push_str(" amb");
    }
    if !comment.is_empty() {
        if let Some(grid) = extract_mic_e_grid_locator(&comment) {
            summary.push_str(&format!(" grid={grid}"));
        }
        if let Some(altitude_feet) = extract_mic_e_altitude_feet(&comment) {
            summary.push_str(&format!(" alt={}ft", altitude_feet));
        }
    }
    if !comment.is_empty() {
        summary.push_str(&format!(" {}", comment));
    }
    Some(summary)
}

/// Decode a Mic-E information field into a compact human-readable summary.
///
/// Returns `None` if the frame is not Mic-E or cannot be decoded.
pub fn decode_mic_e(parsed: &ParsedUiFrame) -> Option<String> {
    // Strip SSID from callsign (AX.25 already separates them but be defensive)
    let dst_call = parsed.destination.callsign.as_str();
    decode_mic_e_from_parts(dst_call, &parsed.info)
}

/// Build an APRS message ACK info field.
///
/// Format: `:{ADDRESSEE:<9}:ack{msgid}`
pub fn build_aprs_ack_info(addressee: &str, msgid: &str) -> Vec<u8> {
    let clean_addressee = addressee.trim().to_uppercase();
    // APRS spec: addressee field must be exactly 9 characters. Truncate if over-length.
    let capped = &clean_addressee[..clean_addressee.len().min(9)];
    let padded = format!("{capped:<9}");
    format!(":{padded}:ack{msgid}").into_bytes()
}

/// Parse "CALL-SSID" into ("CALL", ssid_u8). Defaults to SSID 0 if not present.
fn split_callsign_ssid(input: &str) -> (String, u8) {
    let s = input.trim().to_uppercase();
    if let Some((call, ssid_str)) = s.split_once('-') {
        let ssid = ssid_str.parse::<u8>().unwrap_or(0).min(15);
        (call.to_string(), ssid)
    } else {
        (s, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_address_callsign_padded() {
        let addr = encode_address("KB1ABC", 0, false, false);
        assert_eq!(addr[0], b'K' << 1);
        assert_eq!(addr[1], b'B' << 1);
        assert_eq!(addr[2], b'1' << 1);
        assert_eq!(addr[3], b'A' << 1);
        assert_eq!(addr[4], b'B' << 1);
        assert_eq!(addr[5], b'C' << 1);
        assert_eq!(addr[6] & 0x01, 0x00); // not last
    }

    #[test]
    fn encode_address_last_bit_set() {
        let addr = encode_address("W1AW", 0, true, false);
        assert_eq!(addr[6] & 0x01, 0x01); // last bit set
    }

    #[test]
    fn encode_address_ssid_encoded() {
        let addr = encode_address("N0CALL", 7, false, false);
        let ssid = (addr[6] >> 1) & 0x0F;
        assert_eq!(ssid, 7);
    }

    #[test]
    fn split_callsign_ssid_with_ssid() {
        let (call, ssid) = split_callsign_ssid("W1AW-9");
        assert_eq!(call, "W1AW");
        assert_eq!(ssid, 9);
    }

    #[test]
    fn split_callsign_ssid_no_ssid() {
        let (call, ssid) = split_callsign_ssid("W1AW");
        assert_eq!(call, "W1AW");
        assert_eq!(ssid, 0);
    }

    #[test]
    fn build_ui_frame_minimum_length() {
        let frame = build_ui_frame("APRS", "W1AW", 0, &[], b"Hello");
        // 7 dest + 7 src + 1 ctrl + 1 pid + 5 info = 21
        assert_eq!(frame.len(), 21);
    }

    #[test]
    fn build_ui_frame_with_digipeater() {
        let frame = build_ui_frame("APRS", "W1AW", 0, &["WIDE1-1"], b"Test");
        // 7 + 7 + 7 digi + 1 ctrl + 1 pid + 4 info = 27
        assert_eq!(frame.len(), 27);
    }

    #[test]
    fn build_ui_frame_ctrl_pid_bytes() {
        let frame = build_ui_frame("APRS", "W1AW", 0, &[], b"Data");
        let ctrl_idx = 14;
        let pid_idx = 15;
        assert_eq!(frame[ctrl_idx], 0x03);
        assert_eq!(frame[pid_idx], 0xF0);
    }

    #[test]
    fn build_ui_frame_src_last_bit_set_no_digis() {
        let frame = build_ui_frame("APRS", "W1AW", 0, &[], b"x");
        // src address ends at byte 13; last bit of byte 13 should be 1
        assert_eq!(frame[13] & 0x01, 0x01);
    }

    #[test]
    fn build_ui_frame_src_last_bit_cleared_with_digis() {
        let frame = build_ui_frame("APRS", "W1AW", 0, &["RELAY"], b"x");
        // src last bit should be 0 when digipeaters follow
        assert_eq!(frame[13] & 0x01, 0x00);
    }

    #[test]
    fn build_aprs_message_info_formats_addressee_and_text() {
        let info = build_aprs_message_info("WXBOT", "36043", None);
        assert_eq!(info, b":WXBOT    :36043".to_vec());
    }

    #[test]
    fn build_aprs_message_info_uppercases_and_pads_to_nine() {
        let info = build_aprs_message_info("wxyo", "test", None);
        assert_eq!(info, b":WXYO     :test".to_vec());
    }

    #[test]
    fn build_aprs_message_info_appends_message_number() {
        let info = build_aprs_message_info("WXYO", "36117", Some("25"));
        assert_eq!(info, b":WXYO     :36117{25".to_vec());
    }

    #[test]
    fn build_aprs_message_info_truncates_addressee_to_nine() {
        // Addressee longer than 9 chars must be truncated so the field is exactly 9.
        let info = build_aprs_message_info("LONGCALL-15", "hi", None);
        let s = std::str::from_utf8(&info).unwrap();
        // Field is ":XXXXXXXXXXX:" — second colon must be at index 10 (9-char field).
        assert_eq!(&s[..1], ":");
        assert_eq!(&s[10..11], ":");
        assert_eq!(&s[1..10].len(), &9usize);
    }

    #[test]
    fn build_aprs_message_info_exactly_nine_stays_intact() {
        // "KB1ABC-15" is 9 chars — should be emitted as-is with no padding or truncation.
        let info = build_aprs_message_info("KB1ABC-15", "test", None);
        assert_eq!(info, b":KB1ABC-15:test".to_vec());
    }

    #[test]
    fn build_aprs_ack_info_truncates_addressee_to_nine() {
        let ack = build_aprs_ack_info("LONGCALL-15", "42");
        let s = std::str::from_utf8(&ack).unwrap();
        assert_eq!(&s[10..11], ":");
        assert_eq!(&s[1..10].len(), &9usize);
    }

    #[test]
    fn parse_ui_frame_round_trips_basic_frame() {
        let frame = build_ui_frame("APRS", "W1AW", 9, &[], b"Hello");
        let parsed = parse_ui_frame(&frame).expect("frame should parse");

        assert_eq!(parsed.destination.callsign, "APRS");
        assert_eq!(parsed.destination.ssid, 0);
        assert_eq!(parsed.source.callsign, "W1AW");
        assert_eq!(parsed.source.ssid, 9);
        assert!(parsed.digipeaters.is_empty());
        assert_eq!(parsed.control, 0x03);
        assert_eq!(parsed.pid, 0xF0);
        assert_eq!(parsed.info, b"Hello");
    }

    #[test]
    fn parse_ui_frame_reads_digipeaters() {
        let frame = build_ui_frame("APRS", "WT1W", 0, &["WIDE1-1", "WIDE2-2"], b"Test");
        let parsed = parse_ui_frame(&frame).expect("frame should parse");

        assert_eq!(parsed.digipeaters.len(), 2);
        assert_eq!(parsed.digipeaters[0].callsign, "WIDE1");
        assert_eq!(parsed.digipeaters[0].ssid, 1);
        assert_eq!(parsed.digipeaters[1].callsign, "WIDE2");
        assert_eq!(parsed.digipeaters[1].ssid, 2);
    }

    #[test]
    fn parse_ui_frame_rejects_short_frame() {
        let error = parse_ui_frame(&[0x00; 8]).expect_err("short frame should fail");
        assert!(error.contains("too short"));
    }

    #[test]
    fn parse_aprs_message_extracts_text_without_msgid() {
        let info = ":WT1W-15  :You around?{1";
        let parsed = parse_aprs_message(info).expect("should parse valid APRS message");
        assert_eq!(parsed.addressee, "WT1W-15");
        assert_eq!(parsed.text, "You around?");
        assert_eq!(parsed.msgid, Some("1".to_string()));
    }

    #[test]
    fn parse_aprs_message_handles_text_without_msgid() {
        let info = ":WXYT     :Hello there";
        let parsed = parse_aprs_message(info).expect("should parse APRS message without ID");
        assert_eq!(parsed.addressee, "WXYT");
        assert_eq!(parsed.text, "Hello there");
        assert_eq!(parsed.msgid, None);
    }

    #[test]
    fn parse_aprs_message_trims_addressee() {
        let info = ":KB1ABC   :Test message{42";
        let parsed = parse_aprs_message(info).expect("should parse and trim addressee");
        assert_eq!(parsed.addressee, "KB1ABC");
        assert_eq!(parsed.text, "Test message");
        assert_eq!(parsed.msgid, Some("42".to_string()));
    }

    #[test]
    fn parse_aprs_message_rejects_invalid_format() {
        // Missing first colon
        assert!(parse_aprs_message("WT1W     :text").is_none());
        
        // Missing second colon at position 10
        assert!(parse_aprs_message(":WT1W    text").is_none());
        
        // Too short
        assert!(parse_aprs_message(":WT1W").is_none());
    }

    #[test]
    fn decode_mic_e_rejects_non_mic_e_payload() {
        let parsed = ParsedUiFrame {
            destination: ParsedAddress { callsign: "APRS".to_string(), ssid: 0, repeated: false },
            source: ParsedAddress { callsign: "W1AW".to_string(), ssid: 0, repeated: false },
            digipeaters: vec![],
            control: 0x03,
            pid: 0xF0,
            info: b":W1AW     :hello".to_vec(),
        };

        assert!(decode_mic_e(&parsed).is_none());
    }

    #[test]
    fn decode_mic_e_extracts_human_summary() {
        let parsed = ParsedUiFrame {
            destination: ParsedAddress { callsign: "TRQP7T".to_string(), ssid: 0, repeated: false },
            source: ParsedAddress { callsign: "N1JCM".to_string(), ssid: 9, repeated: false },
            digipeaters: vec![],
            control: 0x03,
            pid: 0xF0,
            info: b"`c'wl|+>/`\"4-}_%".to_vec(),
        };

        let decoded = decode_mic_e(&parsed).expect("expected mic-e decode");
        assert!(decoded.starts_with("Mic-E pos "));
        assert!(decoded.contains("sym=/>"));
        assert!(decoded.contains("comment=M0 Off Duty"));
    }

    #[test]
    fn decode_mic_e_extracts_grid_and_altitude_from_status_text() {
        let parsed = ParsedUiFrame {
            destination: ParsedAddress { callsign: "TRQP7T".to_string(), ssid: 0, repeated: false },
            source: ParsedAddress { callsign: "N1JCM".to_string(), ssid: 9, repeated: false },
            digipeaters: vec![],
            control: 0x03,
            pid: 0xF0,
            info: b"`c'wl|+>/>IO91SX/G Hello /A=001234".to_vec(),
        };

        let decoded = decode_mic_e(&parsed).expect("expected mic-e decode");
        assert!(decoded.contains("grid=IO91SX"));
        assert!(decoded.contains("alt=1234ft"));
        assert!(decoded.contains(">IO91SX/G Hello /A=001234"));
    }

    #[test]
    fn build_aprs_position_info_never_emits_60_minutes() {
        // 42.999999° rounds to 43°00.00', NOT 42°60.00' (invalid per spec).
        let info = build_aprs_position_info(42.999999, -71.499999, '/', '-', "");
        let s = std::str::from_utf8(&info).unwrap();
        assert_eq!(s, "!4300.00N/07130.00W-");

        // Longitude carry: 179.999999° -> 180°00.00'.
        let info = build_aprs_position_info(0.5, 179.999999, '/', '>', "");
        let s = std::str::from_utf8(&info).unwrap();
        assert_eq!(s, "!0030.00N/18000.00E>");
    }

    #[test]
    fn build_aprs_position_info_normal_case_unchanged() {
        let info = build_aprs_position_info(42.5, -71.25, '/', '-', "home");
        let s = std::str::from_utf8(&info).unwrap();
        assert_eq!(s, "!4230.00N/07115.00W-home");

        // Southern / western hemispheres.
        let info = build_aprs_position_info(-33.86, 151.21, '\\', '>', "");
        let s = std::str::from_utf8(&info).unwrap();
        assert_eq!(s, "!3351.60S\\15112.60E>");
    }

    #[test]
    fn build_aprs_message_info_rejects_invalid_msgid() {
        // Over 5 chars: dropped, sent without msgid.
        let info = build_aprs_message_info("WXYO", "hi", Some("123456"));
        assert_eq!(info, b":WXYO     :hi".to_vec());

        // Non-alphanumeric: dropped.
        let info = build_aprs_message_info("WXYO", "hi", Some("1@2"));
        assert_eq!(info, b":WXYO     :hi".to_vec());

        // Empty: no msgid (unchanged behavior).
        let info = build_aprs_message_info("WXYO", "hi", Some(""));
        assert_eq!(info, b":WXYO     :hi".to_vec());
    }

    #[test]
    fn build_aprs_message_info_accepts_reply_ack_msgid_forms() {
        // Reply-ack capable forms must survive validation.
        let info = build_aprs_message_info("WXYO", "hi", Some("12}"));
        assert_eq!(info, b":WXYO     :hi{12}".to_vec());

        let info = build_aprs_message_info("WXYO", "hi", Some("12}34"));
        assert_eq!(info, b":WXYO     :hi{12}34".to_vec());

        // Over-long reply-ack part: dropped entirely.
        let info = build_aprs_message_info("WXYO", "hi", Some("12}345678"));
        assert_eq!(info, b":WXYO     :hi".to_vec());
    }

    #[test]
    fn parse_aprs_message_non_ascii_does_not_panic() {
        // Multi-byte UTF-8 in the addressee field: byte 9 is a continuation
        // byte, so this is rejected — the point is that it must not panic.
        assert!(parse_aprs_message(":ééééé    :text").is_none());

        // Multi-byte UTF-8 in the body parses fine.
        let parsed = parse_aprs_message(":WT1W     :héllo{1").expect("should parse");
        assert_eq!(parsed.text, "héllo");
        assert_eq!(parsed.msgid, Some("1".to_string()));
    }

    #[test]
    fn parse_aprs_message_probe_matrix() {
        let reject = [
            "",                 // empty
            ":",                // bare colon
            ":WT1W",            // too short for the 9-char addressee field
            "WT1W     :text",   // missing leading colon
            ":WT1W    text",    // second colon not at byte 9
            ":WT1W     x",      // byte 9 present but not ':'
        ];
        for input in reject {
            assert!(parse_aprs_message(input).is_none(), "should reject: {input:?}");
        }

        // Minimal valid message, no msgid
        let p = parse_aprs_message(":W1AW-15  :hello").unwrap();
        assert_eq!(p.addressee, "W1AW-15");
        assert_eq!(p.text, "hello");
        assert_eq!(p.msgid, None);

        // Empty body after the second colon
        let p = parse_aprs_message(":W1AW     :").unwrap();
        assert_eq!(p.text, "");

        // '{' inside the text: the LAST brace delimits the msgid
        let p = parse_aprs_message(":W1AW     :a{b}c{12").unwrap();
        assert_eq!(p.text, "a{b}c");
        assert_eq!(p.msgid, Some("12".to_string()));

        // Trailing brace = empty msgid marker (reply-ack capable, no id)
        let p = parse_aprs_message(":W1AW     :text{").unwrap();
        assert_eq!(p.text, "text");
        assert_eq!(p.msgid, Some("".to_string()));

        // RX is lenient about over-long msgids; validation lives TX-side
        let p = parse_aprs_message(":W1AW     :text{123456789").unwrap();
        assert_eq!(p.msgid, Some("123456789".to_string()));

        // Very long body still parses
        let long = format!(":W1AW     :{}", "x".repeat(500));
        assert!(parse_aprs_message(&long).is_some());
    }

    #[test]
    fn parse_tnc2_protocol_view_probe_matrix() {
        use crate::tnc::parse_tnc2_protocol_view;

        assert!(parse_tnc2_protocol_view("").is_none());
        assert!(parse_tnc2_protocol_view("NOADDRSEP").is_none());
        assert!(parse_tnc2_protocol_view("SRC>DST-no-colon").is_none());

        let (s, d, path, info) = parse_tnc2_protocol_view("SRC>DST:info").unwrap();
        assert_eq!((s.as_str(), d.as_str(), info.as_str()), ("SRC", "DST", "info"));
        assert!(path.is_empty());

        let (s, d, path, info) =
            parse_tnc2_protocol_view("A1B2C-9>APWOLF,WIDE1-1,WIDE2-2*::W1AW     :hi{1").unwrap();
        assert_eq!(s, "A1B2C-9");
        assert_eq!(d, "APWOLF");
        assert_eq!(path, vec!["WIDE1-1".to_string(), "WIDE2-2*".to_string()]);
        assert_eq!(info, ":W1AW     :hi{1");
    }

    #[test]
    fn decode_mic_e_probe_matrix() {
        let frame = |dst: &str, info: &[u8]| ParsedUiFrame {
            destination: ParsedAddress { callsign: dst.to_string(), ssid: 0, repeated: false },
            source: ParsedAddress { callsign: "N1JCM".to_string(), ssid: 9, repeated: false },
            digipeaters: vec![],
            control: 0x03,
            pid: 0xF0,
            info: info.to_vec(),
        };

        // Info field shorter than the 9-byte Mic-E minimum
        assert!(decode_mic_e(&frame("TRQP7T", b"`abc")).is_none());

        // Leading byte not a Mic-E type ('`' or '\'')
        assert!(decode_mic_e(&frame("TRQP7T", b"!c'wl|+>/")).is_none());

        // Ambiguity characters (K/L/Z) in the destination decode but flag amb
        let decoded = decode_mic_e(&frame("TRKP7T", b"`c'wl|+>/`\"4-}_%")).expect("should decode");
        assert!(decoded.contains("amb"));
    }

    #[test]
    fn build_aprs_position_info_edge_cases() {
        let encode = |lat: f64, lon: f64| {
            String::from_utf8(build_aprs_position_info(lat, lon, '/', '-', "")).unwrap()
        };

        // Equator / prime meridian
        assert_eq!(encode(0.0, 0.0), "!0000.00N/00000.00E-");

        // Tiny negatives: correct hemisphere, no negative-zero weirdness
        assert_eq!(encode(-0.0001, -0.0001), "!0000.01S/00000.01W-");

        // Near-boundary carry clamps at the poles / antimeridian
        assert_eq!(encode(89.999999, -179.999999), "!9000.00N/18000.00W-");

        // Exact boundaries are representable
        assert_eq!(encode(90.0, 180.0), "!9000.00N/18000.00E-");
        assert_eq!(encode(-90.0, -180.0), "!9000.00S/18000.00W-");
    }
}
