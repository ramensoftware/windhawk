//! The codec: a `u32` little-endian payload length followed by that many bytes
//! of UTF-8 JSON.
//!
//! Framing is explicit rather than `PIPE_TYPE_MESSAGE` so the codec is testable
//! over any `Read`/`Write` pair, and so a partial write is a plain resumable
//! loop rather than a message-mode error.
//!
//! The size cap is a PARAMETER of every entry point here, never a constant in
//! this crate. The value a Windhawk channel needs is derived from the largest
//! payload the core's contract accepts, which this crate must not know; a
//! constant that happened to be right today would drift from that contract the
//! first time it moved.
//!
//! The cap is enforced on the WRITE side as well as the read side, and the two
//! sides do different things with a violation. A frame that is too large to
//! write is never emitted: the caller fails that one request locally, with the
//! size in the error, which is diagnosable. A frame that is too large to read
//! closes the channel, because the request id lives inside the payload that
//! would have to be skipped - there is nothing to attribute the failure to -
//! and because a length above a cap the writer enforces is a bug or a corrupted
//! stream rather than a payload. The cap still does its first job in both
//! directions: no length a peer sends can make this side allocate arbitrarily.

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The bytes a frame costs on the wire beyond its payload.
pub const FRAME_HEADER_BYTES: usize = 4;

/// A frame that could not be written, read, or converted to or from its value.
#[derive(Debug)]
pub enum FrameError {
    /// The payload exceeds the cap. On the write side this fails one request and
    /// nothing reaches the wire; on the read side the caller closes the channel.
    TooLarge { bytes: usize, cap: usize },
    /// The stream ended cleanly, between frames. Not an error at the channel
    /// level - it is how a peer that has exited announces itself.
    Eof,
    /// The stream ended in the middle of a frame, or the transport failed.
    Io(io::Error),
    /// The payload is not the JSON the frame type expects.
    Json(serde_json::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooLarge { bytes, cap } => {
                write!(f, "frame of {bytes} bytes exceeds the {cap} byte cap")
            }
            FrameError::Eof => write!(f, "the channel ended between frames"),
            FrameError::Io(error) => write!(f, "channel I/O failed: {error}"),
            FrameError::Json(error) => write!(f, "frame payload is not valid JSON: {error}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        FrameError::Io(error)
    }
}

/// Serialize `value` into a complete frame (header included), refusing one that
/// would exceed `cap`.
pub fn encode<T: Serialize>(value: &T, cap: usize) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(value).map_err(FrameError::Json)?;
    if payload.len() > cap {
        return Err(FrameError::TooLarge {
            bytes: payload.len(),
            cap,
        });
    }
    // The length prefix is a u32, so a cap above u32::MAX could not be expressed
    // on the wire whatever the payload. Checked here rather than trusted from the
    // caller, since the caller derives its cap from a contract constant. The cap
    // reported is the wire's own, not the caller's: the caller's was cleared by
    // the check above, and an error naming a cap the payload is under says
    // nothing a reader can act on.
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge {
        bytes: payload.len(),
        cap: u32::MAX as usize,
    })?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Parse a frame payload (no header) into its value.
pub fn decode<T: DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(payload).map_err(FrameError::Json)
}

/// Write one frame. The whole frame is built before any byte goes out, so a
/// writer holding a lock emits a frame atomically rather than interleaving a
/// header with another thread's payload.
pub fn write_frame<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    cap: usize,
) -> Result<(), FrameError> {
    let frame = encode(value, cap)?;
    writer.write_all(&frame)?;
    Ok(())
}

/// Read one frame. Returns [`FrameError::Eof`] when the stream ends cleanly
/// between frames, and [`FrameError::Io`] when it ends inside one.
pub fn read_frame<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    cap: usize,
) -> Result<T, FrameError> {
    let mut header = [0u8; FRAME_HEADER_BYTES];
    if !fill(reader, &mut header)? {
        return Err(FrameError::Eof);
    }
    let length = u32::from_le_bytes(header) as usize;
    if length > cap {
        // Deliberately no allocation and no skip: the caller closes the channel.
        // A skip would need the very length this branch has caught being wrong.
        return Err(FrameError::TooLarge { bytes: length, cap });
    }

    let mut payload = vec![0u8; length];
    if !fill(reader, &mut payload)? {
        return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()));
    }
    decode(&payload)
}

/// Fill `buf` completely. Returns `Ok(false)` for a clean end of stream before
/// the first byte, and an `UnexpectedEof` error for one after it - the caller
/// needs to tell "the peer exited" from "the peer was cut off mid-frame".
fn fill<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, io::Error> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Toy {
        id: u64,
        text: String,
    }

    fn toy(text: &str) -> Toy {
        Toy {
            id: 7,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_frame_round_trips_through_a_byte_stream() {
        let mut stream = Vec::new();
        write_frame(&mut stream, &toy("hello"), 1024).unwrap();
        write_frame(&mut stream, &toy("again"), 1024).unwrap();

        let mut reader = stream.as_slice();
        assert_eq!(
            read_frame::<_, Toy>(&mut reader, 1024).unwrap(),
            toy("hello")
        );
        assert_eq!(
            read_frame::<_, Toy>(&mut reader, 1024).unwrap(),
            toy("again")
        );
        assert!(matches!(
            read_frame::<_, Toy>(&mut reader, 1024),
            Err(FrameError::Eof)
        ));
    }

    #[test]
    fn a_stream_that_ends_inside_a_frame_is_not_a_clean_end() {
        let mut stream = Vec::new();
        write_frame(&mut stream, &toy("truncated"), 1024).unwrap();
        stream.truncate(stream.len() - 3);

        let mut reader = stream.as_slice();
        let error = read_frame::<_, Toy>(&mut reader, 1024).unwrap_err();
        match error {
            FrameError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof),
            other => panic!("expected a truncated read, got {other:?}"),
        }
    }

    #[test]
    fn a_header_that_ends_early_is_also_a_truncation() {
        let mut reader = [1u8, 0].as_slice();
        assert!(matches!(
            read_frame::<_, Toy>(&mut reader, 1024),
            Err(FrameError::Io(_))
        ));
    }

    #[test]
    fn an_over_cap_payload_is_refused_by_the_writer_and_never_reaches_the_stream() {
        let mut stream = Vec::new();
        let error = write_frame(&mut stream, &toy(&"x".repeat(200)), 64).unwrap_err();
        match error {
            FrameError::TooLarge { bytes, cap } => {
                assert!(bytes > 64, "the error names the size that was refused");
                assert_eq!(cap, 64);
            }
            other => panic!("expected an over-cap refusal, got {other:?}"),
        }
        assert!(
            stream.is_empty(),
            "an over-cap frame must not be partly emitted"
        );
    }

    #[test]
    fn an_over_cap_length_is_refused_by_the_reader_without_allocating_it() {
        // A header claiming 4 GiB, with no payload behind it. The reader must
        // report the cap violation off the length alone.
        let mut stream = u32::MAX.to_le_bytes().to_vec();
        stream.extend_from_slice(b"nothing like that many bytes");

        let mut reader = stream.as_slice();
        match read_frame::<_, Toy>(&mut reader, 64).unwrap_err() {
            FrameError::TooLarge { bytes, cap } => {
                assert_eq!(bytes, u32::MAX as usize);
                assert_eq!(cap, 64);
            }
            other => panic!("expected an over-cap refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_payload_that_is_not_the_frame_type_is_a_json_error() {
        let mut stream = Vec::new();
        write_frame(&mut stream, &serde_json::json!({"nope": true}), 1024).unwrap();

        let mut reader = stream.as_slice();
        assert!(matches!(
            read_frame::<_, Toy>(&mut reader, 1024),
            Err(FrameError::Json(_))
        ));
    }

    #[test]
    fn a_payload_exactly_at_the_cap_is_admitted() {
        let payload = serde_json::to_vec(&toy("fits")).unwrap();
        let cap = payload.len();

        let mut stream = Vec::new();
        write_frame(&mut stream, &toy("fits"), cap).unwrap();
        assert_eq!(stream.len(), FRAME_HEADER_BYTES + cap);

        let mut reader = stream.as_slice();
        assert_eq!(read_frame::<_, Toy>(&mut reader, cap).unwrap(), toy("fits"));
    }
}
