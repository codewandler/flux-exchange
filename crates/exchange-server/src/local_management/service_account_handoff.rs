//! Exact one-frame Service Account credential handoff.
//!
//! Capability validation and transport-specific transfer live outside this codec. This module
//! admits only the bytes that may cross an already validated one-way writer capability, and never
//! interprets or formats the opaque token payload.

use std::fmt;

#[cfg(unix)]
pub(super) mod unix_transfer;

const MAGIC: &[u8; 4] = b"FXSA";
const VERSION: u8 = 1;
const DIRECTION_EXCHANGE_TO_WRITER: u8 = 1;
const HEADER_LEN: usize = 12;
const MIN_TOKEN_BYTES: usize = 1;
const MAX_TOKEN_BYTES: usize = 512;
const MAX_FRAME_BYTES: usize = HEADER_LEN + MAX_TOKEN_BYTES;

/// One opaque Service Account credential handoff.
///
/// Deliberately has no `Debug`, `Display`, serialization or string conversion implementation.
pub(super) struct HandoffFrame {
    token: Vec<u8>,
}

impl HandoffFrame {
    /// Construct one bounded opaque token payload.
    pub(super) fn new(token: Vec<u8>) -> Result<Self, HandoffError> {
        admit_token_length(token.len())?;
        Ok(Self { token })
    }

    /// The opaque bytes for the already-authorized receiving store.
    pub(super) fn token(&self) -> &[u8] {
        &self.token
    }

    /// Encode the exact `exchange.service-account-handoff.v1` frame.
    pub(super) fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(HEADER_LEN + self.token.len());
        encoded.extend_from_slice(MAGIC);
        encoded.push(VERSION);
        encoded.push(DIRECTION_EXCHANGE_TO_WRITER);
        encoded.extend_from_slice(&[0, 0]);
        // The constructor keeps this far below u32::MAX.
        encoded.extend_from_slice(&(self.token.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&self.token);
        encoded
    }
}

/// Incremental receiver for exactly one FXSA frame followed by EOF.
///
/// A complete frame is not released before [`finish`](Self::finish): EOF is part of the contract,
/// and accepting the payload earlier would make trailing bytes or a second frame invisible.
pub(super) struct HandoffDecoder {
    state: DecoderState,
}

enum DecoderState {
    Reading { bytes: Vec<u8>, received: usize },
    Finished,
    Refused,
}

impl HandoffDecoder {
    pub(super) const fn new() -> Self {
        Self {
            state: DecoderState::Reading {
                bytes: Vec::new(),
                received: 0,
            },
        }
    }

    /// Add arbitrarily split or coalesced byte-stream input.
    pub(super) fn push(&mut self, input: &[u8]) -> Result<(), HandoffError> {
        let DecoderState::Reading { bytes, received } = &mut self.state else {
            return Err(HandoffError::TerminalState);
        };
        *received = received
            .checked_add(input.len())
            .ok_or(HandoffError::SurplusData {
                expected: MAX_FRAME_BYTES,
                received: usize::MAX,
            })?;

        // Retain only enough input to classify every frame. The caller already owns `input`; the
        // decoder never creates an unbounded second copy for a deceptive length or surplus body.
        let retain = (MAX_FRAME_BYTES + 1).saturating_sub(bytes.len());
        bytes.extend_from_slice(&input[..input.len().min(retain)]);
        match inspect(bytes, *received) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.state = DecoderState::Refused;
                Err(error)
            }
        }
    }

    /// Accept EOF and release the sole opaque frame.
    pub(super) fn finish(&mut self) -> Result<HandoffFrame, HandoffError> {
        let prior = std::mem::replace(&mut self.state, DecoderState::Refused);
        let DecoderState::Reading { bytes, received } = prior else {
            return Err(HandoffError::TerminalState);
        };
        if bytes.len() < HEADER_LEN {
            return Err(HandoffError::TruncatedFrame {
                expected: HEADER_LEN,
                received,
            });
        }
        let header = parse_header(&bytes)?;
        let expected = HEADER_LEN + header.payload_length;
        if received < expected {
            return Err(HandoffError::TruncatedFrame { expected, received });
        }
        if received > expected {
            return Err(HandoffError::SurplusData { expected, received });
        }
        let frame = HandoffFrame::new(bytes[HEADER_LEN..expected].to_vec())?;
        self.state = DecoderState::Finished;
        Ok(frame)
    }
}

impl Default for HandoffDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// One-shot writer state for an already validated write-only capability.
pub(super) struct HandoffWriter {
    state: WriterState,
}

enum WriterState {
    Ready,
    Written,
    Refused,
}

impl HandoffWriter {
    pub(super) const fn new() -> Self {
        Self {
            state: WriterState::Ready,
        }
    }

    /// Write exactly one complete frame. A partial or failed sink is terminal and never retried on
    /// this capability because the receiver cannot distinguish a replay from a second credential.
    pub(super) fn write<W: std::io::Write>(
        &mut self,
        sink: &mut W,
        frame: &HandoffFrame,
    ) -> Result<(), HandoffError> {
        if !matches!(self.state, WriterState::Ready) {
            return Err(HandoffError::TerminalState);
        }
        self.state = WriterState::Refused;
        sink.write_all(&frame.encode())
            .map_err(|_| HandoffError::SinkFailure)?;
        self.state = WriterState::Written;
        Ok(())
    }

    /// Whether the whole frame reached the sink. This does not claim receiver persistence.
    pub(super) const fn frame_written(&self) -> bool {
        matches!(self.state, WriterState::Written)
    }
}

impl Default for HandoffWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Value-free refusal from the closed FXSA byte/state contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HandoffError {
    InvalidMagic,
    UnsupportedVersion(u8),
    WrongDirection(u8),
    NonzeroFlags([u8; 2]),
    InvalidLength { declared: usize },
    TruncatedFrame { expected: usize, received: usize },
    SurplusData { expected: usize, received: usize },
    SinkFailure,
    TerminalState,
}

impl fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("invalid FXSA magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported FXSA version {version}")
            }
            Self::WrongDirection(direction) => {
                write!(formatter, "wrong FXSA direction {direction}")
            }
            Self::NonzeroFlags(flags) => write!(
                formatter,
                "FXSA reserved flags must be zero, received {:02x}{:02x}",
                flags[0], flags[1]
            ),
            Self::InvalidLength { declared } => {
                write!(formatter, "FXSA token length {declared} is outside 1..=512")
            }
            Self::TruncatedFrame { expected, received } => write!(
                formatter,
                "truncated FXSA frame: expected {expected} bytes, received {received}"
            ),
            Self::SurplusData { expected, received } => write!(
                formatter,
                "surplus FXSA data: expected {expected} bytes, received {received}"
            ),
            Self::SinkFailure => formatter.write_str("FXSA writer capability refused the frame"),
            Self::TerminalState => formatter.write_str("FXSA handoff is already terminal"),
        }
    }
}

impl std::error::Error for HandoffError {}

struct Header {
    payload_length: usize,
}

fn inspect(bytes: &[u8], received: usize) -> Result<(), HandoffError> {
    if bytes.len() < HEADER_LEN {
        return Ok(());
    }
    let header = parse_header(bytes)?;
    let expected = HEADER_LEN + header.payload_length;
    if received > expected {
        return Err(HandoffError::SurplusData { expected, received });
    }
    Ok(())
}

fn parse_header(bytes: &[u8]) -> Result<Header, HandoffError> {
    debug_assert!(bytes.len() >= HEADER_LEN);
    if &bytes[..4] != MAGIC {
        return Err(HandoffError::InvalidMagic);
    }
    if bytes[4] != VERSION {
        return Err(HandoffError::UnsupportedVersion(bytes[4]));
    }
    if bytes[5] != DIRECTION_EXCHANGE_TO_WRITER {
        return Err(HandoffError::WrongDirection(bytes[5]));
    }
    if bytes[6..8] != [0, 0] {
        return Err(HandoffError::NonzeroFlags([bytes[6], bytes[7]]));
    }
    let payload_length = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    admit_token_length(payload_length)?;
    Ok(Header { payload_length })
}

fn admit_token_length(length: usize) -> Result<(), HandoffError> {
    if (MIN_TOKEN_BYTES..=MAX_TOKEN_BYTES).contains(&length) {
        Ok(())
    } else {
        Err(HandoffError::InvalidLength { declared: length })
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{HandoffDecoder, HandoffError, HandoffFrame, HandoffWriter, HEADER_LEN};

    #[test]
    fn frame_has_the_exact_fxsa_header_and_opaque_payload() {
        let frame = HandoffFrame::new(vec![0x00, 0xff]).expect("opaque token frame");
        assert_eq!(
            frame.encode(),
            b"FXSA\x01\x01\x00\x00\x00\x00\x00\x02\x00\xff"
        );
        assert_eq!(frame.token(), &[0x00, 0xff]);

        let maximum = HandoffFrame::new(vec![0xa5; 512]).expect("maximum token frame");
        assert_eq!(maximum.encode().len(), HEADER_LEN + 512);
        assert_eq!(&maximum.encode()[8..12], &[0, 0, 2, 0]);
        assert_eq!(
            HandoffFrame::new(Vec::new()).err(),
            Some(HandoffError::InvalidLength { declared: 0 })
        );
        assert_eq!(
            HandoffFrame::new(vec![0; 513]).err(),
            Some(HandoffError::InvalidLength { declared: 513 })
        );
    }

    #[test]
    fn receiver_accepts_every_split_and_requires_eof() {
        let encoded = HandoffFrame::new(vec![0x00, 0xff, b'F', b'X', b'S', b'A'])
            .expect("prefix-independent opaque token")
            .encode();
        for split in 0..=encoded.len() {
            let mut decoder = HandoffDecoder::new();
            decoder.push(&encoded[..split]).expect("valid prefix");
            decoder.push(&encoded[split..]).expect("valid suffix");
            let decoded = decoder.finish().expect("exact frame plus EOF");
            assert_eq!(decoded.token(), &[0x00, 0xff, b'F', b'X', b'S', b'A']);
        }
    }

    #[test]
    fn header_mutations_refuse_before_payload_release() {
        let valid = HandoffFrame::new(vec![0x42]).expect("frame").encode();
        let cases = [
            (0, b'B', HandoffError::InvalidMagic),
            (4, 2, HandoffError::UnsupportedVersion(2)),
            (5, 0, HandoffError::WrongDirection(0)),
            (5, 2, HandoffError::WrongDirection(2)),
            (6, 1, HandoffError::NonzeroFlags([1, 0])),
            (7, 1, HandoffError::NonzeroFlags([0, 1])),
        ];
        for (offset, value, expected) in cases {
            let mut mutation = valid.clone();
            mutation[offset] = value;
            let mut decoder = HandoffDecoder::new();
            assert_eq!(decoder.push(&mutation), Err(expected));
            assert_eq!(decoder.finish().err(), Some(HandoffError::TerminalState));
        }
    }

    #[test]
    fn deceptive_lengths_refuse_without_waiting_or_allocating() {
        for (length, expected) in [
            (0_u32, HandoffError::InvalidLength { declared: 0 }),
            (513, HandoffError::InvalidLength { declared: 513 }),
            (
                u32::MAX,
                HandoffError::InvalidLength {
                    declared: u32::MAX as usize,
                },
            ),
        ] {
            let mut header = *b"FXSA\x01\x01\x00\x00\x00\x00\x00\x01";
            header[8..12].copy_from_slice(&length.to_be_bytes());
            let mut decoder = HandoffDecoder::new();
            assert_eq!(decoder.push(&header), Err(expected));
        }
    }

    #[test]
    fn every_early_eof_is_truncated_and_releases_no_token() {
        let encoded = HandoffFrame::new(vec![0x5a; 32]).expect("frame").encode();
        for end in 0..encoded.len() {
            let mut decoder = HandoffDecoder::new();
            decoder.push(&encoded[..end]).expect("valid prefix");
            let expected = if end < HEADER_LEN {
                HEADER_LEN
            } else {
                encoded.len()
            };
            assert_eq!(
                decoder.finish().err(),
                Some(HandoffError::TruncatedFrame {
                    expected,
                    received: end,
                })
            );
        }
    }

    #[test]
    fn trailing_byte_coalesced_second_frame_and_post_terminal_input_refuse() {
        let encoded = HandoffFrame::new(vec![0x42]).expect("frame").encode();
        for surplus in [vec![0], encoded.clone()] {
            let mut combined = encoded.clone();
            combined.extend_from_slice(&surplus);
            let mut decoder = HandoffDecoder::new();
            assert_eq!(
                decoder.push(&combined),
                Err(HandoffError::SurplusData {
                    expected: encoded.len(),
                    received: combined.len(),
                })
            );
            assert_eq!(decoder.push(&[]), Err(HandoffError::TerminalState));
        }

        let mut trailing = HandoffDecoder::new();
        trailing
            .push(&encoded)
            .expect("one complete frame buffered");
        assert_eq!(
            trailing.push(&[0]),
            Err(HandoffError::SurplusData {
                expected: encoded.len(),
                received: encoded.len() + 1,
            })
        );

        let mut decoder = HandoffDecoder::new();
        decoder.push(&encoded).expect("one frame");
        decoder.finish().expect("one frame plus EOF");
        assert_eq!(decoder.push(&[]), Err(HandoffError::TerminalState));
        assert_eq!(decoder.finish().err(), Some(HandoffError::TerminalState));
    }

    #[test]
    fn one_shot_writer_reports_only_complete_frame_delivery() {
        let frame = HandoffFrame::new(vec![0xde, 0xad, 0xbe, 0xef]).expect("frame");
        let mut sink = Vec::new();
        let mut writer = HandoffWriter::new();
        writer.write(&mut sink, &frame).expect("complete sink");
        assert!(writer.frame_written());
        assert_eq!(sink, frame.encode());
        assert_eq!(
            writer.write(&mut sink, &frame),
            Err(HandoffError::TerminalState)
        );

        let mut failed = HandoffWriter::new();
        assert_eq!(
            failed.write(&mut FailAfter { remaining: 5 }, &frame),
            Err(HandoffError::SinkFailure)
        );
        assert!(!failed.frame_written());
        assert_eq!(
            failed.write(&mut Vec::new(), &frame),
            Err(HandoffError::TerminalState)
        );
    }

    struct FailAfter {
        remaining: usize,
    }

    impl io::Write for FailAfter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
            }
            let written = bytes.len().min(self.remaining);
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn errors_and_source_never_embed_opaque_payload_bytes() {
        let source = include_str!("service_account_handoff.rs");
        let forbidden = [
            ["String::", "from_utf8"].concat(),
            ["from_utf8", "_lossy"].concat(),
            ["token", " ="].concat(),
            ["%", "token"].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "source contains `{forbidden}`"
            );
        }
        let error = HandoffError::SinkFailure;
        assert!(!format!("{error:?} {error}").contains("deadbeef"));
    }
}
