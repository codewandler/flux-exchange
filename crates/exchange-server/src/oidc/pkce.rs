//! PKCE (RFC 7636): the proof that the client redeeming an authorization code is the one that
//! asked for it.
//!
//! # What it is for here
//!
//! The authorization code comes back to us through the **browser**, in a URL. Anything that can see
//! that URL — browser history, a proxy log, a referrer header, another application registered for
//! the same redirect — has the code. PKCE makes the code useless on its own: whoever redeems it must
//! also present the `code_verifier`, which never left this process.
//!
//! That matters even for a confidential client holding a secret, which is what this host is. The
//! secret protects the token endpoint against a stranger; PKCE protects it against *this* code being
//! replayed by whatever else saw the redirect. They answer different questions, and RFC 9700
//! (OAuth 2.0 Security Best Current Practice) asks for both.
//!
//! # `S256` only
//!
//! [`METHOD`] is `S256` and there is no way to select `plain`. Under `plain` the challenge *is* the
//! verifier, so anything that could read the authorization request has already read the secret and
//! the mechanism protects nothing. RFC 7636 §4.2 requires `S256` of any client capable of it, and
//! offering a weaker spelling as an option is how a deployment ends up on it.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::entropy;

/// The code challenge method this host uses, in the spelling the authorization request carries.
pub const METHOD: &str = "S256";

/// How many bytes of entropy a code verifier carries.
///
/// 32 bytes — 256 bits — which base64url-encodes to 43 characters. RFC 7636 §4.1 permits 43 to 128
/// characters and recommends 32 bytes of randomness, so this is the low end of the length range and
/// the full strength of the recommendation.
const VERIFIER_BYTES: usize = 32;

/// A PKCE code verifier: the secret half, held here until the code is redeemed.
///
/// It is a bearer credential for the duration of one sign-in — whoever holds it can redeem the
/// matching authorization code — so it follows [`SessionToken`](crate::session::SessionToken): no
/// `Display`, and a `Debug` that redacts. A verifier in a log line is an authorization code anyone
/// reading the log can spend.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier(String);

impl Verifier {
    /// Draw a fresh verifier from the operating system's randomness.
    pub fn generate() -> Result<Self, std::io::Error> {
        // base64url of 32 random bytes is 43 characters and uses only unreserved characters, which
        // is exactly the `code_verifier` grammar in RFC 7636 §4.1 — so no escaping question ever
        // arises on the way to the token endpoint.
        Ok(Self(base64url(&entropy::bytes::<VERIFIER_BYTES>()?)))
    }

    /// The verifier as it goes to the token endpoint. Every call site is a deliberate disclosure.
    ///
    /// Reached only through [`Redemption`](super::exchange::Redemption), which `HttpTokenExchange`
    /// spends at the token endpoint.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The challenge that goes in the authorization request — `BASE64URL(SHA256(verifier))`.
    ///
    /// Derived rather than stored, so a verifier and its challenge cannot drift apart.
    ///
    /// `sha2` since the X-04 dependency decision opened it up. This was a hand-written SHA-256 for
    /// as long as no digest crate was allowed in; RFC 7636 Appendix A's vector below is unchanged
    /// and still passes, which is what makes the swap checkable rather than merely plausible.
    pub fn challenge(&self) -> Challenge {
        Challenge(base64url(&Sha256::digest(self.0.as_bytes())))
    }
}

impl fmt::Debug for Verifier {
    /// Redacts. See the type's own documentation for why.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Verifier(redacted)")
    }
}

/// A PKCE code challenge: the public half, safe to put in a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Challenge(String);

impl Challenge {
    /// The challenge as it goes in the authorization request.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// base64url without padding (RFC 4648 §5), which is what RFC 7636 §4.2 specifies.
///
/// Written out for the same reason the cookie parser in [`session`](crate::session) is: the
/// workspace carries no base64 crate, and this direction of the transform is a 20-line table
/// lookup with published vectors rather than a grammar with edge cases.
///
/// `pub(super)` so [`http_exchange`](super::http_exchange)'s tests hand-assemble a JWT with the one
/// encoder this module already proves against RFC 4648's vectors. A second copy in a test module
/// would be a second thing to keep correct, and a forgery test built on a broken encoder passes for
/// the wrong reason.
pub(super) fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        // Pack the chunk into the low 24 bits, left-aligned, so a short final chunk simply has
        // fewer output characters rather than a special case.
        let packed = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |packed, (index, byte)| {
                packed | (u32::from(*byte) << (16 - 8 * index))
            });

        // 3 bytes make 4 characters, 2 make 3, 1 makes 2. No `=` padding: RFC 7636 omits it.
        for index in 0..=chunk.len() {
            let sextet = (packed >> (18 - 6 * index)) & 0b11_1111;
            encoded.push(ALPHABET[sextet as usize] as char);
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    /// **RFC 7636 Appendix A**, the specification's own worked example.
    ///
    /// This is the assertion that matters most in this module, because it pins the whole chain a
    /// provider re-computes on its side — SHA-256, then base64url, then the exact characters — in
    /// one comparison. If `sha2` or `base64url` were wrong in any way that mattered, this fails.
    #[test]
    fn the_rfc_7636_worked_example() {
        let verifier = Verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());

        assert_eq!(
            verifier.challenge().as_str(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        );
    }

    /// base64url's own vectors (RFC 4648 §10, transposed to the URL alphabet and unpadded), so a
    /// failure points at the encoder rather than at the hash.
    ///
    /// The last case is the one that matters for a URL: byte values that encode to `-` and `_`
    /// under this alphabet and to `+` and `/` under the standard one. Getting that wrong would
    /// produce a challenge that survives our tests and is mangled in transit.
    #[test]
    fn base64url_encodes_without_padding_and_without_plus_or_slash() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");

        assert_eq!(base64url(&[0xfb, 0xef, 0xbe]), "----");
        assert_eq!(base64url(&[0xff, 0xff, 0xff]), "____");
    }

    /// The verifier is a secret, so it must not print itself.
    #[test]
    fn a_verifier_redacts_itself() {
        let verifier = Verifier::generate().expect("the OS has randomness");

        let printed = format!("{verifier:?}");
        assert!(!printed.contains(verifier.as_str()), "{printed}");
        assert_eq!(printed, "Verifier(redacted)");
    }

    /// Two sign-ins must never share a verifier, and each must satisfy RFC 7636 §4.1's grammar —
    /// 43 to 128 unreserved characters. A verifier the token endpoint refuses to parse is a sign-in
    /// that fails at the last step, which is the failure mode this story is told not to ship.
    #[test]
    fn every_verifier_is_distinct_and_well_formed() {
        let drawn: BTreeSet<String> = (0..64)
            .map(|_| {
                Verifier::generate()
                    .expect("the OS has randomness")
                    .as_str()
                    .to_string()
            })
            .collect();

        assert_eq!(drawn.len(), 64, "verifiers must not repeat");

        for verifier in &drawn {
            assert_eq!(verifier.len(), 43, "32 bytes, base64url, unpadded");
            assert!(
                verifier
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '.' | '_' | '~')),
                "a code verifier is unreserved characters only: {verifier}",
            );
        }
    }

    /// The challenge is a function of the verifier, so a different verifier is a different
    /// challenge. Without this, a `challenge()` that returned a constant would satisfy every other
    /// assertion here except the RFC vector.
    #[test]
    fn a_challenge_follows_its_verifier() {
        let one = Verifier::generate().expect("the OS has randomness");
        let two = Verifier::generate().expect("the OS has randomness");

        assert_ne!(one.challenge(), two.challenge());
        assert_eq!(one.challenge(), one.challenge(), "and is deterministic");
        assert_ne!(
            one.challenge().as_str(),
            one.as_str(),
            "S256 must not hand back the verifier, which is what `plain` would do",
        );
    }
}
