//! Randomness from the operating system, and the refusal when there is none.
//!
//! One source, read one way, for everything in this binary that needs an unguessable value: session
//! tokens, OIDC `state`, OIDC `nonce`, and the PKCE code verifier. They are four names for the same
//! requirement — a value an attacker cannot predict — and a second entropy path is how one of them
//! quietly becomes weaker than the others.
//!
//! **Refuse; never repair.** If the source is unreadable the caller gets an error and nothing is
//! minted. There is no weaker fallback worth having: a value from a predictable source is a value
//! anyone can guess, and a host that quietly downgraded to one would look exactly like a host that
//! had not.

use std::fs::File;
use std::io::Read;

/// Where every unguessable value in this binary comes from.
///
/// Named rather than inlined so a refusal can tell an operator which file to look at.
pub const SOURCE: &str = "/dev/urandom";

/// `N` bytes from [`SOURCE`], or the reason they could not be read.
pub fn bytes<const N: usize>() -> Result<[u8; N], std::io::Error> {
    let mut bytes = [0_u8; N];

    File::open(SOURCE).and_then(|mut source| source.read_exact(&mut bytes))?;

    Ok(bytes)
}

/// `N` bytes from [`SOURCE`], hex encoded — so `2 * N` characters.
///
/// Hex rather than base64: a lookup table of two lines instead of one of sixty-four, and none of
/// the values here is long enough for the density to be worth anything. Hex is also unreserved in
/// a URL query, which matters for `state` and `nonce`.
pub fn hex<const N: usize>() -> Result<String, std::io::Error> {
    Ok(encode_hex(&bytes::<N>()?))
}

/// Hex encoding, lower case.
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    #[test]
    fn hex_encoding_is_lower_case_and_two_characters_a_byte() {
        assert_eq!(encode_hex(&[0x00, 0x0f, 0xf0, 0xff]), "000ff0ff");
        assert_eq!(encode_hex(&[]), "");
    }

    /// The property every caller of this module depends on. Sixty-four draws of 32 bytes colliding
    /// even once would mean the source is not what it claims to be.
    #[test]
    fn every_draw_is_distinct_and_the_declared_length() {
        let drawn: BTreeSet<String> = (0..64)
            .map(|_| hex::<32>().expect("the OS has randomness"))
            .collect();

        assert_eq!(drawn.len(), 64, "drawn values must not repeat");
        for value in &drawn {
            assert_eq!(value.len(), 64, "32 bytes, hex encoded");
        }
    }

    /// A zero-filled draw would pass the length assertion above while carrying no entropy at all.
    #[test]
    fn a_draw_is_not_all_zeroes() {
        let drawn = bytes::<32>().expect("the OS has randomness");
        assert!(drawn.iter().any(|byte| *byte != 0), "{drawn:?}");
    }
}
