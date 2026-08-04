use std::fmt;

/// Bytes before an FXLM payload. There is deliberately no flags field.
pub(super) const HEADER_LEN: usize = 12;

const MAGIC: &[u8; 4] = b"FXLM";
const VERSION: u8 = 1;
const MAX_CONTROL_BYTES: usize = 65_536;
const MAX_SECRET_BYTES: usize = 8_192;

/// The side that emitted a frame, encoded directly into the FXLM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Direction {
    ClientToServer = 1,
    ServerToClient = 2,
}

impl TryFrom<u8> for Direction {
    type Error = FrameError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ClientToServer),
            2 => Ok(Self::ServerToClient),
            other => Err(FrameError::UnknownDirection(other)),
        }
    }
}

/// The exhaustive opcode vocabulary of `exchange.local-management.v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(super) enum Opcode {
    ConnectBegin = 0x0001,
    NeedSecrets = 0x0002,
    Secret = 0x0003,
    ConnectCommit = 0x0004,
    ConnectQuery = 0x0005,
    ConnectReceipt = 0x0006,
    PlanQuery = 0x0007,
    PlanResponse = 0x0008,
    GrantPreview = 0x0010,
    GrantCandidate = 0x0011,
    GrantApply = 0x0012,
    GrantQuery = 0x0013,
    GrantReceipt = 0x0014,
    ServiceAccountMint = 0x0020,
    ServiceAccountQuery = 0x0021,
    ServiceAccountReceipt = 0x0022,
    CredentialBegin = 0x0030,
    CredentialCommit = 0x0031,
    CredentialReceipt = 0x0032,
    CredentialQuery = 0x0033,
    Error = 0x7fff,
}

impl TryFrom<u16> for Opcode {
    type Error = FrameError;

    fn try_from(value: u16) -> Result<Self, FrameError> {
        match value {
            0x0001 => Ok(Self::ConnectBegin),
            0x0002 => Ok(Self::NeedSecrets),
            0x0003 => Ok(Self::Secret),
            0x0004 => Ok(Self::ConnectCommit),
            0x0005 => Ok(Self::ConnectQuery),
            0x0006 => Ok(Self::ConnectReceipt),
            0x0007 => Ok(Self::PlanQuery),
            0x0008 => Ok(Self::PlanResponse),
            0x0010 => Ok(Self::GrantPreview),
            0x0011 => Ok(Self::GrantCandidate),
            0x0012 => Ok(Self::GrantApply),
            0x0013 => Ok(Self::GrantQuery),
            0x0014 => Ok(Self::GrantReceipt),
            0x0020 => Ok(Self::ServiceAccountMint),
            0x0021 => Ok(Self::ServiceAccountQuery),
            0x0022 => Ok(Self::ServiceAccountReceipt),
            0x0030 => Ok(Self::CredentialBegin),
            0x0031 => Ok(Self::CredentialCommit),
            0x0032 => Ok(Self::CredentialReceipt),
            0x0033 => Ok(Self::CredentialQuery),
            0x7fff => Ok(Self::Error),
            other => Err(FrameError::UnknownOpcode(other)),
        }
    }
}

/// A parsed frame. Payload bytes intentionally have no `Debug` implementation.
pub(super) struct Frame {
    direction: Direction,
    opcode: Opcode,
    payload: Payload,
}

enum Payload {
    Control(Vec<u8>),
    Secret(SecretPayload),
}

/// Secret bytes and their one-based requested ordinal.
pub(super) struct SecretPayload {
    ordinal: u16,
    bytes: Vec<u8>,
}

impl SecretPayload {
    pub(super) const fn ordinal(&self) -> u16 {
        self.ordinal
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Frame {
    pub(super) fn control(
        direction: Direction,
        opcode: Opcode,
        payload: Vec<u8>,
    ) -> Result<Self, FrameError> {
        if opcode == Opcode::Secret {
            return Err(FrameError::UnexpectedOpcode(opcode));
        }
        admit_length(opcode, payload.len())?;
        Ok(Self {
            direction,
            opcode,
            payload: Payload::Control(payload),
        })
    }

    pub(super) fn secret(
        direction: Direction,
        ordinal: u16,
        bytes: Vec<u8>,
    ) -> Result<Self, FrameError> {
        if ordinal == 0 {
            return Err(FrameError::InvalidSecretOrdinal);
        }
        if !(1..=MAX_SECRET_BYTES).contains(&bytes.len()) {
            return Err(FrameError::InvalidSecretLength {
                declared: bytes.len(),
            });
        }
        Ok(Self {
            direction,
            opcode: Opcode::Secret,
            payload: Payload::Secret(SecretPayload { ordinal, bytes }),
        })
    }

    pub(super) const fn direction(&self) -> Direction {
        self.direction
    }

    pub(super) const fn opcode(&self) -> Opcode {
        self.opcode
    }

    pub(super) fn control_payload(&self) -> Option<&[u8]> {
        match &self.payload {
            Payload::Control(bytes) => Some(bytes),
            Payload::Secret(_) => None,
        }
    }

    pub(super) fn secret_payload(&self) -> Option<&SecretPayload> {
        match &self.payload {
            Payload::Control(_) => None,
            Payload::Secret(secret) => Some(secret),
        }
    }

    pub(super) fn encode(self) -> Vec<u8> {
        let payload_length = match &self.payload {
            Payload::Control(bytes) => bytes.len(),
            Payload::Secret(secret) => 2 + secret.bytes.len(),
        };
        // Both payload classes have bounds far below u32::MAX.
        let payload_length = payload_length as u32;
        let mut encoded = Vec::with_capacity(HEADER_LEN + payload_length as usize);
        encoded.extend_from_slice(MAGIC);
        encoded.push(VERSION);
        encoded.push(self.direction as u8);
        encoded.extend_from_slice(&(self.opcode as u16).to_be_bytes());
        encoded.extend_from_slice(&payload_length.to_be_bytes());
        match self.payload {
            Payload::Control(bytes) => encoded.extend_from_slice(&bytes),
            Payload::Secret(secret) => {
                encoded.extend_from_slice(&secret.ordinal.to_be_bytes());
                encoded.extend_from_slice(&secret.bytes);
            }
        }
        encoded
    }

    fn decode(direction: Direction, opcode: Opcode, payload: Vec<u8>) -> Result<Self, FrameError> {
        if opcode != Opcode::Secret {
            return Self::control(direction, opcode, payload);
        }
        let Some(ordinal_bytes) = payload.get(..2) else {
            return Err(FrameError::InvalidSecretLength { declared: 0 });
        };
        let ordinal = u16::from_be_bytes([ordinal_bytes[0], ordinal_bytes[1]]);
        Self::secret(direction, ordinal, payload[2..].to_vec())
    }
}

/// Value-free refusals produced before any local-management state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameError {
    InvalidMagic,
    UnsupportedVersion(u8),
    UnknownDirection(u8),
    WrongDirection {
        expected: Direction,
        actual: Direction,
    },
    UnknownOpcode(u16),
    UnexpectedOpcode(Opcode),
    FrameTooLarge {
        opcode: Opcode,
        declared: usize,
        maximum: usize,
    },
    InvalidSecretOrdinal,
    InvalidSecretLength {
        declared: usize,
    },
    TruncatedFrame {
        expected: usize,
        received: usize,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("invalid FXLM magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported FXLM version {version}")
            }
            Self::UnknownDirection(direction) => {
                write!(formatter, "unknown FXLM direction {direction}")
            }
            Self::WrongDirection { expected, actual } => write!(
                formatter,
                "wrong FXLM direction: expected {}, received {}",
                *expected as u8, *actual as u8
            ),
            Self::UnknownOpcode(opcode) => write!(formatter, "unknown FXLM opcode {opcode:#06x}"),
            Self::UnexpectedOpcode(opcode) => {
                write!(formatter, "unexpected FXLM opcode {:#06x}", *opcode as u16)
            }
            Self::FrameTooLarge {
                opcode,
                declared,
                maximum,
            } => write!(
                formatter,
                "FXLM opcode {:#06x} declares {declared} bytes above maximum {maximum}",
                *opcode as u16
            ),
            Self::InvalidSecretOrdinal => {
                formatter.write_str("FXLM secret ordinal must be nonzero")
            }
            Self::InvalidSecretLength { declared } => {
                write!(
                    formatter,
                    "FXLM secret length {declared} is outside 1..=8192"
                )
            }
            Self::TruncatedFrame { expected, received } => write!(
                formatter,
                "truncated FXLM frame: expected {expected} bytes, received {received}"
            ),
        }
    }
}

impl std::error::Error for FrameError {}

/// Incremental native byte-stream decoder. Message boundaries have no meaning here.
pub(super) struct StreamDecoder {
    expected_direction: Direction,
    buffered: Vec<u8>,
}

impl StreamDecoder {
    pub(super) const fn new(expected_direction: Direction) -> Self {
        Self {
            expected_direction,
            buffered: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        self.buffered.extend_from_slice(bytes);
        Ok(())
    }

    pub(super) fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let Some(header) = self.header()? else {
            return Ok(None);
        };
        let total = HEADER_LEN + header.payload_length;
        if self.buffered.len() < total {
            return Ok(None);
        }
        let payload = self.buffered[HEADER_LEN..total].to_vec();
        self.buffered.drain(..total);
        Frame::decode(header.direction, header.opcode, payload).map(Some)
    }

    pub(super) fn finish(&mut self) -> Result<(), FrameError> {
        if self.buffered.is_empty() {
            return Ok(());
        }
        let expected = match self.header()? {
            Some(header) => HEADER_LEN + header.payload_length,
            None => HEADER_LEN,
        };
        Err(FrameError::TruncatedFrame {
            expected,
            received: self.buffered.len(),
        })
    }

    fn header(&self) -> Result<Option<Header>, FrameError> {
        if self.buffered.len() < HEADER_LEN {
            return Ok(None);
        }
        if &self.buffered[..4] != MAGIC {
            return Err(FrameError::InvalidMagic);
        }
        if self.buffered[4] != VERSION {
            return Err(FrameError::UnsupportedVersion(self.buffered[4]));
        }
        let direction = Direction::try_from(self.buffered[5])?;
        if direction != self.expected_direction {
            return Err(FrameError::WrongDirection {
                expected: self.expected_direction,
                actual: direction,
            });
        }
        let opcode = Opcode::try_from(u16::from_be_bytes([self.buffered[6], self.buffered[7]]))?;
        let payload_length = u32::from_be_bytes([
            self.buffered[8],
            self.buffered[9],
            self.buffered[10],
            self.buffered[11],
        ]) as usize;
        admit_length(opcode, payload_length)?;
        Ok(Some(Header {
            direction,
            opcode,
            payload_length,
        }))
    }
}

struct Header {
    direction: Direction,
    opcode: Opcode,
    payload_length: usize,
}

fn admit_length(opcode: Opcode, payload_length: usize) -> Result<(), FrameError> {
    if opcode == Opcode::Secret {
        let secret_length = payload_length.saturating_sub(2);
        if !(1..=MAX_SECRET_BYTES).contains(&secret_length) || payload_length < 3 {
            return Err(FrameError::InvalidSecretLength {
                declared: secret_length,
            });
        }
        return Ok(());
    }
    if payload_length > MAX_CONTROL_BYTES {
        return Err(FrameError::FrameTooLarge {
            opcode,
            declared: payload_length,
            maximum: MAX_CONTROL_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Direction, Frame, FrameError, Opcode, StreamDecoder, HEADER_LEN};

    fn control_json(length: usize) -> Vec<u8> {
        assert!(length >= 2);
        let mut json = Vec::with_capacity(length);
        json.push(b'"');
        json.resize(length - 1, b'x');
        json.push(b'"');
        json
    }

    fn encoded_control(direction: Direction, opcode: Opcode, payload: &[u8]) -> Vec<u8> {
        Frame::control(direction, opcode, payload.to_vec())
            .expect("control frame")
            .encode()
    }

    #[test]
    fn header_is_exactly_twelve_bytes_with_no_flag_field() {
        let encoded = encoded_control(Direction::ClientToServer, Opcode::ConnectBegin, b"{}");
        assert_eq!(HEADER_LEN, 12);
        assert_eq!(
            &encoded[..HEADER_LEN],
            &[b'F', b'X', b'L', b'M', 1, 1, 0, 1, 0, 0, 0, 2]
        );
        assert_eq!(&encoded[HEADER_LEN..], b"{}");

        let response = encoded_control(Direction::ServerToClient, Opcode::Error, b"null");
        assert_eq!(
            &response[..HEADER_LEN],
            &[b'F', b'X', b'L', b'M', 1, 2, 0x7f, 0xff, 0, 0, 0, 4]
        );

        let wider_length = encoded_control(
            Direction::ClientToServer,
            Opcode::PlanQuery,
            &control_json(258),
        );
        assert_eq!(&wider_length[8..12], &[0, 0, 1, 2]);
    }

    #[test]
    fn opcode_values_are_the_exact_closed_big_endian_vocabulary() {
        let controls: [(Opcode, u16); 20] = [
            (Opcode::ConnectBegin, 0x0001),
            (Opcode::NeedSecrets, 0x0002),
            (Opcode::ConnectCommit, 0x0004),
            (Opcode::ConnectQuery, 0x0005),
            (Opcode::ConnectReceipt, 0x0006),
            (Opcode::PlanQuery, 0x0007),
            (Opcode::PlanResponse, 0x0008),
            (Opcode::GrantPreview, 0x0010),
            (Opcode::GrantCandidate, 0x0011),
            (Opcode::GrantApply, 0x0012),
            (Opcode::GrantQuery, 0x0013),
            (Opcode::GrantReceipt, 0x0014),
            (Opcode::ServiceAccountMint, 0x0020),
            (Opcode::ServiceAccountQuery, 0x0021),
            (Opcode::ServiceAccountReceipt, 0x0022),
            (Opcode::CredentialBegin, 0x0030),
            (Opcode::CredentialCommit, 0x0031),
            (Opcode::CredentialReceipt, 0x0032),
            (Opcode::CredentialQuery, 0x0033),
            (Opcode::Error, 0x7fff),
        ];
        for (opcode, value) in controls {
            let encoded = encoded_control(Direction::ClientToServer, opcode, b"null");
            assert_eq!(&encoded[6..8], &value.to_be_bytes());
            let mut decoder = StreamDecoder::new(Direction::ClientToServer);
            decoder.push(&encoded).expect("bounded opcode frame");
            assert_eq!(
                decoder
                    .next_frame()
                    .expect("known opcode")
                    .expect("complete frame")
                    .opcode(),
                opcode
            );
        }

        let secret = Frame::secret(Direction::ClientToServer, 1, vec![1])
            .expect("secret frame")
            .encode();
        assert_eq!(&secret[6..8], &0x0003_u16.to_be_bytes());
    }

    #[test]
    fn native_stream_accepts_a_split_at_every_byte_boundary() {
        let encoded = encoded_control(
            Direction::ClientToServer,
            Opcode::GrantPreview,
            br#"{"connector":"gitlab"}"#,
        );
        for split in 0..=encoded.len() {
            let mut decoder = StreamDecoder::new(Direction::ClientToServer);
            decoder.push(&encoded[..split]).expect("prefix is bounded");
            if split < encoded.len() {
                assert!(decoder.next_frame().expect("prefix is valid").is_none());
            }
            decoder.push(&encoded[split..]).expect("suffix is bounded");
            let frame = decoder
                .next_frame()
                .expect("complete frame is valid")
                .expect("frame is complete");
            assert_eq!(frame.direction(), Direction::ClientToServer);
            assert_eq!(frame.opcode(), Opcode::GrantPreview);
            assert_eq!(
                frame.control_payload(),
                Some(br#"{"connector":"gitlab"}"#.as_slice())
            );
            assert!(decoder.next_frame().expect("stream is empty").is_none());
            decoder.finish().expect("frame ended exactly");
        }
    }

    #[test]
    fn native_stream_accepts_byte_reads_and_coalesced_frames() {
        let first = encoded_control(Direction::ClientToServer, Opcode::PlanQuery, b"{}");
        let second = encoded_control(Direction::ClientToServer, Opcode::ConnectQuery, b"null");

        let mut bytewise = StreamDecoder::new(Direction::ClientToServer);
        for byte in &first {
            bytewise.push(&[*byte]).expect("one bounded byte");
        }
        assert_eq!(
            bytewise
                .next_frame()
                .expect("valid frame")
                .expect("complete frame")
                .opcode(),
            Opcode::PlanQuery
        );

        let mut coalesced = StreamDecoder::new(Direction::ClientToServer);
        let mut bytes = first;
        bytes.extend_from_slice(&second);
        coalesced.push(&bytes).expect("two bounded frames");
        assert_eq!(
            coalesced
                .next_frame()
                .expect("first frame")
                .expect("first complete")
                .opcode(),
            Opcode::PlanQuery
        );
        assert_eq!(
            coalesced
                .next_frame()
                .expect("second frame")
                .expect("second complete")
                .opcode(),
            Opcode::ConnectQuery
        );
        assert!(coalesced.next_frame().expect("stream is empty").is_none());
    }

    #[test]
    fn control_json_has_the_exact_65536_byte_bound() {
        let maximum = control_json(65_536);
        let encoded = encoded_control(Direction::ClientToServer, Opcode::CredentialBegin, &maximum);
        assert_eq!(encoded.len(), HEADER_LEN + 65_536);

        let refusal = frame_error(Frame::control(
            Direction::ClientToServer,
            Opcode::CredentialBegin,
            control_json(65_537),
        ));
        assert_eq!(
            refusal,
            FrameError::FrameTooLarge {
                opcode: Opcode::CredentialBegin,
                declared: 65_537,
                maximum: 65_536,
            }
        );
    }

    #[test]
    fn secret_payload_is_ordinal_then_one_to_8192_raw_bytes() {
        let one = Frame::secret(Direction::ClientToServer, 1, vec![0x42])
            .expect("one-byte secret")
            .encode();
        assert_eq!(&one[..HEADER_LEN], b"FXLM\x01\x01\x00\x03\x00\x00\x00\x03");
        assert_eq!(&one[HEADER_LEN..], &[0, 1, 0x42]);

        let maximum = Frame::secret(Direction::ClientToServer, u16::MAX, vec![0xa5; 8192])
            .expect("maximum secret")
            .encode();
        let mut decoder = StreamDecoder::new(Direction::ClientToServer);
        decoder.push(&maximum).expect("bounded secret frame");
        let frame = decoder
            .next_frame()
            .expect("valid secret")
            .expect("complete secret");
        let secret = frame.secret_payload().expect("secret payload");
        assert_eq!(secret.ordinal(), u16::MAX);
        assert_eq!(secret.bytes(), vec![0xa5; 8192].as_slice());

        assert_eq!(
            frame_error(Frame::secret(Direction::ClientToServer, 0, vec![1])),
            FrameError::InvalidSecretOrdinal
        );
        assert_eq!(
            frame_error(Frame::secret(Direction::ClientToServer, 1, Vec::new())),
            FrameError::InvalidSecretLength { declared: 0 }
        );
        assert_eq!(
            frame_error(Frame::secret(Direction::ClientToServer, 1, vec![0; 8193])),
            FrameError::InvalidSecretLength { declared: 8193 }
        );
    }

    #[test]
    fn decoder_refuses_magic_version_direction_opcode_and_deceptive_lengths() {
        let valid = encoded_control(Direction::ClientToServer, Opcode::ConnectCommit, b"{}");

        let mut bad_magic = valid.clone();
        bad_magic[0] = b'B';
        assert_eq!(decode_error(&bad_magic), FrameError::InvalidMagic);

        let mut bad_version = valid.clone();
        bad_version[4] = 2;
        assert_eq!(
            decode_error(&bad_version),
            FrameError::UnsupportedVersion(2)
        );

        let wrong_direction =
            encoded_control(Direction::ServerToClient, Opcode::ConnectReceipt, b"{}");
        assert_eq!(
            decode_error(&wrong_direction),
            FrameError::WrongDirection {
                expected: Direction::ClientToServer,
                actual: Direction::ServerToClient,
            }
        );

        let mut unknown_opcode = valid.clone();
        unknown_opcode[6..8].copy_from_slice(&0x0009_u16.to_be_bytes());
        assert_eq!(
            decode_error(&unknown_opcode),
            FrameError::UnknownOpcode(0x0009)
        );

        let mut oversized_control = valid.clone();
        oversized_control[8..12].copy_from_slice(&65_537_u32.to_be_bytes());
        assert_eq!(
            decode_error(&oversized_control),
            FrameError::FrameTooLarge {
                opcode: Opcode::ConnectCommit,
                declared: 65_537,
                maximum: 65_536,
            }
        );

        let mut empty_secret = Frame::secret(Direction::ClientToServer, 1, vec![1])
            .expect("secret")
            .encode();
        empty_secret[8..12].copy_from_slice(&2_u32.to_be_bytes());
        empty_secret.truncate(HEADER_LEN + 2);
        assert_eq!(
            decode_error(&empty_secret),
            FrameError::InvalidSecretLength { declared: 0 }
        );
    }

    #[test]
    fn eof_refuses_every_truncated_header_and_payload() {
        let encoded = encoded_control(Direction::ClientToServer, Opcode::ServiceAccountMint, b"{}");
        for end in 1..encoded.len() {
            let mut decoder = StreamDecoder::new(Direction::ClientToServer);
            decoder
                .push(&encoded[..end])
                .expect("prefix remains bounded");
            assert!(decoder.next_frame().expect("prefix parses").is_none());
            assert_eq!(
                decoder.finish().expect_err("EOF before frame end refuses"),
                FrameError::TruncatedFrame {
                    expected: if end < HEADER_LEN {
                        HEADER_LEN
                    } else {
                        encoded.len()
                    },
                    received: end,
                }
            );
        }
    }

    #[test]
    fn codec_source_has_no_payload_logging_or_flag_field() {
        let source = include_str!("codec.rs");
        let forbidden = [
            ["tracing", "::"].concat(),
            ["log", "::"].concat(),
            ["print", "ln!"].concat(),
            ["eprint", "ln!"].concat(),
            ["flags", ":"].concat(),
            ["flag", ":"].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "codec must not contain payload logging or a flag field: {forbidden}"
            );
        }
    }

    fn decode_error(bytes: &[u8]) -> FrameError {
        let mut decoder = StreamDecoder::new(Direction::ClientToServer);
        decoder.push(bytes).expect("input copied");
        next_error(decoder.next_frame())
    }

    fn frame_error(result: Result<Frame, FrameError>) -> FrameError {
        match result {
            Ok(_) => panic!("invalid frame construction must refuse"),
            Err(error) => error,
        }
    }

    fn next_error(result: Result<Option<Frame>, FrameError>) -> FrameError {
        match result {
            Ok(_) => panic!("invalid frame must refuse"),
            Err(error) => error,
        }
    }
}
