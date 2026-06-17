use anyhow::{Context, Result, anyhow, bail};
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_ATTACH_PORT: u32 = 50_426;
pub const DETACH_BYTE: u8 = 0x1d; // Ctrl-]

const MAX_PAYLOAD_LEN: usize = 16 * 1024 * 1024;
const TAG_HELLO: u8 = 1;
const TAG_ATTACH: u8 = 2;
const TAG_DATA: u8 = 3;
const TAG_RESIZE: u8 = 4;
const TAG_DETACH: u8 = 5;
const TAG_EXIT: u8 = 6;
const TAG_ERROR: u8 = 7;
const TAG_BUSY: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Hello { version: u16 },
    Attach,
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Detach,
    Exit { code: i32 },
    Error(String),
    Busy,
}

impl Frame {
    fn tag(&self) -> u8 {
        match self {
            Self::Hello { .. } => TAG_HELLO,
            Self::Attach => TAG_ATTACH,
            Self::Data(_) => TAG_DATA,
            Self::Resize { .. } => TAG_RESIZE,
            Self::Detach => TAG_DETACH,
            Self::Exit { .. } => TAG_EXIT,
            Self::Error(_) => TAG_ERROR,
            Self::Busy => TAG_BUSY,
        }
    }

    fn payload(&self) -> Vec<u8> {
        match self {
            Self::Hello { version } => version.to_be_bytes().to_vec(),
            Self::Attach | Self::Detach | Self::Busy => Vec::new(),
            Self::Data(data) => data.clone(),
            Self::Resize { rows, cols } => [rows.to_be_bytes(), cols.to_be_bytes()].concat(),
            Self::Exit { code } => code.to_be_bytes().to_vec(),
            Self::Error(message) => message.as_bytes().to_vec(),
        }
    }
}

pub fn write_frame(writer: &mut impl Write, frame: &Frame) -> Result<()> {
    let payload = frame.payload();
    if payload.len() > MAX_PAYLOAD_LEN {
        bail!("loftd attach protocol payload is too large");
    }
    writer.write_all(&[frame.tag()])?;
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame(reader: &mut impl Read) -> Result<Option<Frame>> {
    let mut header = [0u8; 5];
    match read_exact_or_clean_eof(reader, &mut header)
        .context("failed to read loftd attach protocol frame header")?
    {
        ReadExactOutcome::Complete => {}
        ReadExactOutcome::CleanEof => return Ok(None),
    }
    let payload_len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        bail!("loftd attach protocol frame payload exceeds maximum length");
    }
    let mut payload = vec![0u8; payload_len];
    reader
        .read_exact(&mut payload)
        .context("failed to read loftd attach protocol frame payload")?;
    decode_frame(header[0], payload).map(Some)
}

enum ReadExactOutcome {
    Complete,
    CleanEof,
}

fn read_exact_or_clean_eof(reader: &mut impl Read, buf: &mut [u8]) -> Result<ReadExactOutcome> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(ReadExactOutcome::CleanEof),
            Ok(0) => bail!("truncated frame header"),
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(ReadExactOutcome::Complete)
}

fn decode_frame(tag: u8, payload: Vec<u8>) -> Result<Frame> {
    match tag {
        TAG_HELLO => {
            if payload.len() != 2 {
                bail!("invalid hello payload length");
            }
            Ok(Frame::Hello {
                version: u16::from_be_bytes([payload[0], payload[1]]),
            })
        }
        TAG_ATTACH => empty_payload(payload, Frame::Attach),
        TAG_DATA => Ok(Frame::Data(payload)),
        TAG_RESIZE => {
            if payload.len() != 4 {
                bail!("invalid resize payload length");
            }
            Ok(Frame::Resize {
                rows: u16::from_be_bytes([payload[0], payload[1]]),
                cols: u16::from_be_bytes([payload[2], payload[3]]),
            })
        }
        TAG_DETACH => empty_payload(payload, Frame::Detach),
        TAG_EXIT => {
            if payload.len() != 4 {
                bail!("invalid exit payload length");
            }
            Ok(Frame::Exit {
                code: i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]),
            })
        }
        TAG_ERROR => Ok(Frame::Error(
            String::from_utf8(payload).map_err(|_| anyhow!("error payload is not utf-8"))?,
        )),
        TAG_BUSY => empty_payload(payload, Frame::Busy),
        _ => bail!("unknown loftd attach protocol frame tag {tag}"),
    }
}

fn empty_payload(payload: Vec<u8>, frame: Frame) -> Result<Frame> {
    if !payload.is_empty() {
        bail!("frame must have an empty payload");
    }
    Ok(frame)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DetachFilter {
    detached: bool,
}

impl DetachFilter {
    pub fn push(&mut self, input: &[u8], output: &mut Vec<u8>) -> bool {
        for byte in input {
            if *byte == DETACH_BYTE {
                self.detached = true;
                return true;
            }
            output.push(*byte);
        }
        false
    }

    pub fn detached(&self) -> bool {
        self.detached
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip() {
        let frames = vec![
            Frame::Hello { version: 1 },
            Frame::Attach,
            Frame::Data(vec![0, 1, 2, 255]),
            Frame::Resize { rows: 24, cols: 80 },
            Frame::Detach,
            Frame::Exit { code: 42 },
            Frame::Error("nope".to_owned()),
            Frame::Busy,
        ];
        let mut bytes = Vec::new();
        for frame in &frames {
            write_frame(&mut bytes, frame).expect("write frame");
        }
        let mut cursor = Cursor::new(bytes);
        for expected in frames {
            assert_eq!(read_frame(&mut cursor).expect("read frame"), Some(expected));
        }
        assert_eq!(read_frame(&mut cursor).expect("eof"), None);
    }

    #[test]
    fn incomplete_payload_fails_deterministically() {
        let mut cursor = Cursor::new(vec![TAG_DATA, 0, 0, 0, 2, 1]);
        let err = read_frame(&mut cursor).expect_err("short payload should fail");
        assert!(format!("{err:#}").contains("payload"));
    }

    #[test]
    fn partial_header_fails_instead_of_detaching() {
        let mut cursor = Cursor::new(vec![TAG_DATA, 0, 0]);
        let err = read_frame(&mut cursor).expect_err("partial header should fail");
        assert!(format!("{err:#}").contains("truncated frame header"));
    }

    #[test]
    fn detach_filter_consumes_ctrl_right_bracket() {
        let mut filter = DetachFilter::default();
        let mut output = Vec::new();
        assert!(filter.push(b"ab\x1dcd", &mut output));
        assert_eq!(output, b"ab");
        assert!(filter.detached());
    }
}
