//! SHA-256, for one purpose: the PKCE `S256` code challenge.
//!
//! # Why this is written out rather than depended on
//!
//! The workspace carries no hash crate and adding one was not this story's to do. That left three
//! options, and only one of them is honest:
//!
//! 1. Use PKCE's `plain` method, where the challenge *is* the verifier. Rejected. RFC 7636 §4.2
//!    says a client that can do `S256` MUST, and for good reason: with `plain`, anything that can
//!    read the authorization request has read the verifier, which is the entire attack PKCE exists
//!    to stop. Shipping it would be shipping a control that only appears to exist — the failure
//!    `docs/designs/identity-and-session.md` records for `HttpOnly`, repeated.
//! 2. Skip PKCE. Rejected for the same reason.
//! 3. Implement the hash, and pin it against published vectors.
//!
//! This is (3), and it is deliberately the *only* cryptography in this repository. Note what it is
//! not: it is **not** signature verification. Verifying an id token means JWKS retrieval, key
//! selection, RSA or ECDSA, and the algorithm-confusion class of bug — that is behind
//! [`TokenExchange`](super::exchange::TokenExchange) and stays there. SHA-256 is a deterministic
//! function of its input with published test vectors, so "is this correct" is a question a test can
//! answer completely, which is exactly what the tests below do.
//!
//! It also fails closed. If this were wrong, the challenge would not match what the provider
//! computes from the verifier and the token exchange would be refused — a broken sign-in, loudly,
//! rather than a weakened one, quietly.
//!
//! **Replace this with `sha2` the moment a dependency can be added.** Nothing outside this module
//! needs to change: [`digest`] is the whole surface.

/// The round constants: the first 32 bits of the fractional parts of the cube roots of the first
/// 64 primes (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The initial hash value: the first 32 bits of the fractional parts of the square roots of the
/// first 8 primes (FIPS 180-4 §5.3.3).
const INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// The SHA-256 digest of `message`.
pub fn digest(message: &[u8]) -> [u8; 32] {
    let mut hash = INITIAL;

    for block in padded(message).chunks_exact(64) {
        compress(&mut hash, block);
    }

    let mut digest = [0_u8; 32];
    for (word, out) in hash.iter().zip(digest.chunks_exact_mut(4)) {
        out.copy_from_slice(&word.to_be_bytes());
    }

    digest
}

/// The message, followed by `0x80`, then zeroes to 56 bytes mod 64, then the bit length as a
/// big-endian `u64` (FIPS 180-4 §5.1.1).
fn padded(message: &[u8]) -> Vec<u8> {
    let mut padded = Vec::with_capacity(message.len() + 72);
    padded.extend_from_slice(message);
    padded.push(0x80);

    while padded.len() % 64 != 56 {
        padded.push(0);
    }

    // A `u64` of bits cannot overflow for any message this host hashes — the longest is a 43
    // character PKCE verifier — but saturating rather than wrapping keeps the failure mode a wrong
    // answer on an impossible input instead of a panic in release and a panic in debug.
    let bits = (message.len() as u64).saturating_mul(8);
    padded.extend_from_slice(&bits.to_be_bytes());

    padded
}

/// One 64-byte block into the running hash (FIPS 180-4 §6.2.2).
fn compress(hash: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0_u32; 64];

    for (word, raw) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    }
    for index in 16..64 {
        let s0 = schedule[index - 15].rotate_right(7)
            ^ schedule[index - 15].rotate_right(18)
            ^ (schedule[index - 15] >> 3);
        let s1 = schedule[index - 2].rotate_right(17)
            ^ schedule[index - 2].rotate_right(19)
            ^ (schedule[index - 2] >> 10);

        schedule[index] = schedule[index - 16]
            .wrapping_add(s0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *hash;

    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(K[index])
            .wrapping_add(schedule[index]);

        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    for (running, round) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *running = running.wrapping_add(round);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(digest: [u8; 32]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The vectors. This is the whole justification for the module existing: correctness here is a
    /// closed question, and these close it.
    ///
    /// The first three are FIPS 180-4's own examples. The last two are the padding edges — 64 bytes
    /// exactly, where the message fills a block and the padding needs one of its own, and 119
    /// bytes, one short of the length suffix no longer fitting. [`padded`] is where a hand-written
    /// SHA-256 is most often wrong and a short message never exercises it.
    ///
    /// Every expectation here was produced by an independent implementation (`sha256sum`), not
    /// written from memory.
    #[test]
    fn the_published_vectors_hash_as_published() {
        let cases: [(&[u8], &str); 5] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            // 64 bytes exactly.
            (
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno",
                "2ff100b36c386c65a1afc462ad53e25479bec9498ed00aa5a04de584bc25301b",
            ),
            // 119 bytes: the last length that still shares its final block with the suffix.
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq\
                  abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq\
                  abcdbcd",
                "bc2e323c7f48747a630ec89fb248549189b47137edd172c93e8f735041bcc4a7",
            ),
        ];

        for (message, expected) in cases {
            assert_eq!(
                hex(digest(message)),
                expected,
                "SHA-256 of {} bytes",
                message.len(),
            );
        }
    }

    /// A message spanning many blocks, checked against the property that makes SHA-256 usable at
    /// all: the digest is a function of the whole input, so changing any single byte changes it.
    ///
    /// This is the padding-and-chaining assertion the short vectors cannot make.
    #[test]
    fn a_long_message_hashes_blockwise() {
        let long: Vec<u8> = (0..1000).map(|index| (index % 251) as u8).collect();
        let baseline = digest(&long);

        for flip in [0_usize, 55, 63, 64, 500, 998, 999] {
            let mut altered = long.clone();
            altered[flip] ^= 0x01;

            assert_ne!(
                digest(&altered),
                baseline,
                "flipping byte {flip} must change the digest",
            );
        }

        // Length extension: the same bytes with one more must not collide either, which is what
        // pins the length suffix in the padding.
        let mut longer = long.clone();
        longer.push(0);
        assert_ne!(digest(&longer), baseline);
    }

    /// `a` repeated a million times — FIPS 180-4's long example, and the only vector here that
    /// exercises a bit length past a single byte's worth of blocks.
    #[test]
    fn the_million_a_vector() {
        let message = vec![b'a'; 1_000_000];

        assert_eq!(
            hex(digest(&message)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        );
    }
}
