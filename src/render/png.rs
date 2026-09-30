//! Minimal PNG image encoder with a hand-written DEFLATE compressor.
//!
//! The display backend streams rendered frames to a browser, and PNG is the
//! one format every browser decodes natively without client-side JavaScript.
//! Pulling in an image codec crate is not an option in this build environment
//! and would be a large dependency for what we need, so this module implements
//! the small subset of the standards that the framebuffer pipeline requires:
//!
//! * DEFLATE with **fixed Huffman codes only** (RFC 1951 §3.2.6), with a
//!   greedy hash-chain LZ77 matcher.
//! * zlib wrapper (RFC 1950) with adler32.
//! * PNG with 8-bit RGB color type 2 and per-row `Up` filtering, which is
//!   cheap to compute and compresses well for typical game imagery
//!   (flat sky, tiled terrain).
//!
//! Unit tests round-trip the encoded stream through a small test-only fixed
//! Huffman inflater, so a regression in the bitstream cannot ship silently.

// ---------------------------------------------------------------------------
// CRC32 (IEEE, reflected) and adler32
// ---------------------------------------------------------------------------

fn crc32_table() -> &'static [u32; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *entry = c;
        }
        table
    })
}

/// Standard CRC-32 (IEEE 802.3), as required by PNG chunks.
pub fn crc32(data: &[u8]) -> u32 {
    let table = crc32_table();
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// zlib adler32 checksum.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------
// LZ77 + fixed-Huffman DEFLATE
// ---------------------------------------------------------------------------

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const WINDOW_SIZE: usize = 32768;
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MAX_HASH_CHAIN: u32 = 64;
const NIL: u32 = u32::MAX;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// LSB-first bit writer as required by DEFLATE.
struct BitWriter {
    out: Vec<u8>,
    bit_buf: u32,
    bit_count: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::with_capacity(4096),
            bit_buf: 0,
            bit_count: 0,
        }
    }

    /// Writes `count` (0..=24) bits of `value`, least-significant bit first.
    #[inline]
    fn write_bits(&mut self, value: u32, count: u8) {
        self.bit_buf |= value << self.bit_count;
        self.bit_count += count;
        while self.bit_count >= 8 {
            self.out.push((self.bit_buf & 0xFF) as u8);
            self.bit_buf >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Writes a Huffman code given in canonical (MSB-first) form. DEFLATE
    /// stores Huffman codes most-significant-bit first, which for an LSB-first
    /// bit stream means the code value must be bit-reversed.
    #[inline]
    fn write_code(&mut self, code: u16, len: u8) {
        let mut reversed = 0u32;
        for i in 0..len {
            reversed |= (((code >> i) & 1) as u32) << (len - 1 - i);
        }
        self.write_bits(reversed, len);
    }

    fn flush(&mut self) {
        if self.bit_count > 0 {
            self.out.push((self.bit_buf & 0xFF) as u8);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }
}

/// Maps a literal/length symbol to its fixed-Huffman code (RFC 1951 §3.2.6).
#[inline]
fn fixed_literal_code(symbol: u16) -> (u16, u8) {
    match symbol {
        0..=143 => (0x30 + symbol, 8),
        144..=255 => (0x190 + (symbol - 144), 9),
        256..=279 => (symbol - 256, 7),
        280..=287 => (0xC0 + (symbol - 280), 8),
        _ => unreachable!("symbol out of range"),
    }
}

#[inline]
fn hash3(data: &[u8], i: usize) -> usize {
    let v = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Returns the fixed-Huffman length symbol and extra-bits value for a match
/// length (`3..=258`).
fn length_symbol(len: usize) -> (usize, u16, u8) {
    debug_assert!((MIN_MATCH..=MAX_MATCH).contains(&len));
    let mut idx = LENGTH_BASE.len() - 1;
    for (i, &base) in LENGTH_BASE.iter().enumerate().rev() {
        if len >= base as usize {
            idx = i;
            break;
        }
    }
    (257 + idx, (len - LENGTH_BASE[idx] as usize) as u16, LENGTH_EXTRA[idx])
}

/// Returns the fixed-Huffman distance symbol and extra-bits value for a
/// backward distance (`1..=32768`).
fn distance_symbol(dist: usize) -> (usize, u16, u8) {
    let mut idx = DIST_BASE.len() - 1;
    for (i, &base) in DIST_BASE.iter().enumerate().rev() {
        if dist >= base as usize {
            idx = i;
            break;
        }
    }
    (idx, (dist - DIST_BASE[idx] as usize) as u16, DIST_EXTRA[idx])
}

/// DEFLATE compressor using greedy LZ77 with hash chains and fixed Huffman
/// codes. Emits a single final block.
pub(crate) fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new();
    // BFINAL = 1, BTYPE = 01 (fixed Huffman).
    bw.write_bits(1, 1);
    bw.write_bits(1, 2);

    let mut head = vec![NIL; HASH_SIZE];
    let mut prev = vec![NIL; data.len()];

    let mut i = 0usize;
    while i < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;

        if i + MIN_MATCH <= data.len() {
            let h = hash3(data, i);
            let mut cand = head[h];
            let mut chain = 0u32;
            while cand != NIL && chain < MAX_HASH_CHAIN {
                let cand_us = cand as usize;
                let dist = i - cand_us;
                if dist > WINDOW_SIZE {
                    break;
                }
                // Only bother extending if it could beat the current best.
                let limit = (data.len() - i).min(MAX_MATCH);
                if best_len < limit && data[cand_us + best_len] == data[i + best_len] {
                    let mut len = 0usize;
                    while len < limit && data[cand_us + len] == data[i + len] {
                        len += 1;
                    }
                    if len > best_len {
                        best_len = len;
                        best_dist = dist;
                        if len == MAX_MATCH {
                            break;
                        }
                    }
                }
                cand = prev[cand_us];
                chain += 1;
            }
        }

        if best_len >= MIN_MATCH {
            let (sym, extra_val, extra_bits) = length_symbol(best_len);
            let (code, code_len) = fixed_literal_code(sym as u16);
            bw.write_code(code, code_len);
            if extra_bits > 0 {
                bw.write_bits(extra_val as u32, extra_bits);
            }
            let (dsym, dextra_val, dextra_bits) = distance_symbol(best_dist);
            bw.write_code(dsym as u16, 5);
            if dextra_bits > 0 {
                bw.write_bits(dextra_val as u32, dextra_bits);
            }
            // Insert every position covered by the match into the hash chains
            // so later positions can still reference into the match. The
            // insert cursor is separate from `i`: `i` must jump the full
            // match length even where inserts near the end are impossible.
            let match_end = i + best_len;
            let insert_end = match_end.min(data.len().saturating_sub(2));
            let mut p = i;
            while p < insert_end {
                let h = hash3(data, p);
                prev[p] = head[h];
                head[h] = p as u32;
                p += 1;
            }
            i = match_end;
        } else {
            let (code, code_len) = fixed_literal_code(data[i] as u16);
            bw.write_code(code, code_len);
            if i + MIN_MATCH <= data.len() {
                let h = hash3(data, i);
                prev[i] = head[h];
                head[h] = i as u32;
            }
            i += 1;
        }
    }

    // End-of-block symbol.
    let (code, code_len) = fixed_literal_code(256);
    bw.write_code(code, code_len);
    bw.flush();
    bw.out
}

// ---------------------------------------------------------------------------
// zlib + PNG containers
// ---------------------------------------------------------------------------

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Encodes an 8-bit RGB image (`rgb` is `width * height * 3` bytes, row-major,
/// top-left origin) as a PNG file.
///
/// Panics if `rgb.len() != width * height * 3` - that is an internal
/// invariant of the render pipeline, not a recoverable runtime condition.
pub fn encode_png_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    assert_eq!(
        rgb.len(),
        (width as usize) * (height as usize) * 3,
        "rgb buffer size mismatch"
    );

    // Apply the `Up` filter per row: filtered[i] = raw[i] - raw_above[i].
    // Row 0 subtracts zeros (i.e. is stored raw).
    let stride = width as usize * 3;
    let mut filtered = Vec::with_capacity(rgb.len() + height as usize);
    for (row, chunk) in rgb.chunks_exact(stride).enumerate() {
        filtered.push(2u8); // filter type: Up
        if row == 0 {
            filtered.extend_from_slice(chunk);
        } else {
            let prev = &rgb[(row - 1) * stride..row * stride];
            for (cur, above) in chunk.iter().zip(prev.iter()) {
                filtered.push(cur.wrapping_sub(*above));
            }
        }
    }

    let compressed = deflate_fixed(&filtered);

    let mut png = Vec::with_capacity(compressed.len() + 128);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: truecolor RGB
    ihdr.push(0); // compression: deflate
    ihdr.push(0); // filter method
    ihdr.push(0); // interlace: none
    push_chunk(&mut png, b"IHDR", &ihdr);

    let mut idat = Vec::with_capacity(compressed.len() + 6);
    idat.push(0x78); // CMF: deflate, 32K window
    idat.push(0x01); // FLG: check bits, no dict, fastest
    idat.extend_from_slice(&compressed);
    idat.extend_from_slice(&adler32(&filtered).to_be_bytes());
    push_chunk(&mut png, b"IDAT", &idat);

    push_chunk(&mut png, b"IEND", &[]);
    png
}

// ---------------------------------------------------------------------------
// Tests: CRC/adler vectors plus a test-only inflater for round-tripping
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn adler32_known_vectors() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn length_and_distance_symbols_cover_valid_ranges() {
        for len in MIN_MATCH..=MAX_MATCH {
            let (sym, extra, bits) = length_symbol(len);
            assert!((257..=285).contains(&sym));
            assert_eq!(LENGTH_BASE[sym - 257] as usize + extra as usize, len);
            assert_eq!(bits, LENGTH_EXTRA[sym - 257]);
        }
        for dist in 1..=WINDOW_SIZE {
            let (sym, extra, bits) = distance_symbol(dist);
            assert!(sym < 30);
            assert_eq!(DIST_BASE[sym] as usize + extra as usize, dist);
            assert_eq!(bits, DIST_EXTRA[sym]);
        }
    }

    /// LSB-first bit reader (test helper).
    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,   // byte position
        bit: u8,      // bit within byte
    }

    impl<'a> BitReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, pos: 0, bit: 0 }
        }

        fn read_bit(&mut self) -> u32 {
            let b = (self.data[self.pos] >> self.bit) & 1;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
            b as u32
        }

        fn read_bits(&mut self, count: u8) -> u32 {
            let mut v = 0u32;
            for i in 0..count {
                v |= self.read_bit() << i;
            }
            v
        }

        fn read_code(&mut self, len: u8) -> u32 {
            // Huffman codes are stored MSB-first.
            let mut v = 0u32;
            for _ in 0..len {
                v = (v << 1) | self.read_bit();
            }
            v
        }

        fn align_byte(&mut self) {
            if self.bit != 0 {
                self.bit = 0;
                self.pos += 1;
            }
        }
    }

    fn fixed_literal_symbol(code: u32, len: u8) -> u16 {
        match len {
            7 => 256 + code as u16,
            8 => {
                if (0xC0..=0xC7).contains(&code) {
                    280 + (code - 0xC0) as u16
                } else {
                    (code - 0x30) as u16
                }
            }
            9 => 144 + (code - 0x190) as u16,
            _ => panic!("invalid fixed literal code length {len}"),
        }
    }

    /// Wraps a raw deflate stream in the zlib container (header + adler32 of
    /// the *uncompressed* data, as the zlib format requires).
    pub(super) fn zlib_wrap(uncompressed: &[u8], deflate: &[u8]) -> Vec<u8> {
        let mut z = Vec::with_capacity(deflate.len() + 6);
        z.push(0x78);
        z.push(0x01);
        z.extend_from_slice(deflate);
        z.extend_from_slice(&adler32(uncompressed).to_be_bytes());
        z
    }

    /// Inflates a zlib stream that contains only stored or fixed-Huffman
    /// blocks (the only kinds this module emits).
    pub(super) fn zlib_inflate(z: &[u8]) -> Vec<u8> {
        assert_eq!(z[0] & 0x0F, 8, "zlib: deflate method");
        assert_eq!(((z[0] as u16) << 8 | z[1] as u16) % 31, 0, "zlib header check");
        let mut r = BitReader::new(&z[2..z.len() - 4]);
        let mut out = Vec::new();

        loop {
            let bfinal = r.read_bit();
            let btype = r.read_bits(2);
            match btype {
                0 => {
                    r.align_byte();
                    let len = u16::from_le_bytes([r.data[r.pos], r.data[r.pos + 1]]) as usize;
                    r.pos += 4; // LEN + NLEN
                    out.extend_from_slice(&r.data[r.pos..r.pos + len]);
                    r.pos += len;
                }
                1 => loop {
                    // Decode one fixed-Huffman symbol by walking code lengths.
                    let mut code = 0u32;
                    let mut len = 0u8;
                    let sym = loop {
                        code = (code << 1) | r.read_bit();
                        len += 1;
                        let valid = match len {
                            7 => code <= 0b0010111,
                            8 => (0b00110000..=0b10111111).contains(&code) || (0b11000000..=0b11000111).contains(&code),
                            9 => (0b100100000..=0b111111111).contains(&code),
                            _ => false,
                        };
                        if valid {
                            break fixed_literal_symbol(code, len);
                        }
                        if len > 9 {
                            panic!("corrupt fixed-huffman stream");
                        }
                    };
                    if sym == 256 {
                        break;
                    }
                    if sym < 256 {
                        out.push(sym as u8);
                        continue;
                    }
                    // Length + distance copy.
                    let lsym = (sym - 257) as usize;
                    let len = LENGTH_BASE[lsym] as usize + r.read_bits(LENGTH_EXTRA[lsym]) as usize;
                    let dsym = r.read_code(5) as usize;
                    let dist = DIST_BASE[dsym] as usize + r.read_bits(DIST_EXTRA[dsym]) as usize;
                    assert!(dist <= out.len(), "distance beyond output");
                    let start = out.len() - dist;
                    for k in 0..len {
                        let b = out[start + k];
                        out.push(b);
                    }
                },
                _ => panic!("dynamic blocks are not emitted by this encoder"),
            }
            if bfinal == 1 {
                break;
            }
        }

        // Verify the adler32 trailer.
        let expect = u32::from_be_bytes([z[z.len() - 4], z[z.len() - 3], z[z.len() - 2], z[z.len() - 1]]);
        assert_eq!(adler32(&out), expect, "adler32 mismatch");
        out
    }

    #[test]
    fn deflate_roundtrip_text() {
        let data = b"hello hello hello hello world world world".repeat(50);
        let compressed = zlib_wrap(&data, &deflate_fixed(&data));
        // Repetitive data must actually compress.
        assert!(compressed.len() < data.len() / 4, "poor ratio: {} -> {}", data.len(), compressed.len());
        assert_eq!(zlib_inflate(&compressed), data);
    }

    #[test]
    fn deflate_roundtrip_binary_and_edge_cases() {
        // Pseudo-random binary data (worst case for LZ).
        let mut prng: u32 = 0x1234_5678;
        let mut noisy = Vec::with_capacity(70_000);
        for _ in 0..70_000 {
            prng = prng.wrapping_mul(1664525).wrapping_add(1013904223);
            noisy.push((prng >> 24) as u8);
        }
        assert_eq!(zlib_inflate(&zlib_wrap(&noisy, &deflate_fixed(&noisy))), noisy);

        assert_eq!(zlib_inflate(&zlib_wrap(b"", &deflate_fixed(b""))), b"");
        assert_eq!(zlib_inflate(&zlib_wrap(b"a", &deflate_fixed(b"a"))), b"a");
        assert_eq!(zlib_inflate(&zlib_wrap(b"aaaaa", &deflate_fixed(b"aaaaa"))), b"aaaaa");
        // Match of exactly 258 bytes plus more data.
        let run = vec![7u8; 600];
        assert_eq!(zlib_inflate(&zlib_wrap(&run, &deflate_fixed(&run))), run);
    }

    #[test]
    fn png_roundtrip_framebuffer() {
        // A 48x32 frame with a sky gradient and a bright "cube" block, the
        // kind of content this engine actually produces.
        let (w, h) = (48u32, 32u32);
        let mut frame = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = if (16..=28).contains(&x) && (10..=20).contains(&y) {
                    (200, 40, 40)
                } else {
                    (40, 40 + (y * 4) as u8, 160)
                };
                frame.extend_from_slice(&[r, g, b]);
            }
        }
        let png = encode_png_rgb(w, h, &frame);

        // Parse the container back.
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut pos = 8usize;
        let mut idat = Vec::new();
        let mut dims = (0u32, 0u32);
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let payload = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            let mut crc_input = kind.to_vec();
            crc_input.extend_from_slice(payload);
            assert_eq!(crc32(&crc_input), crc, "chunk {} crc", String::from_utf8_lossy(kind));
            match kind {
                b"IHDR" => {
                    dims = (
                        u32::from_be_bytes(payload[0..4].try_into().unwrap()),
                        u32::from_be_bytes(payload[4..8].try_into().unwrap()),
                    );
                    assert_eq!(payload[8], 8);
                    assert_eq!(payload[9], 2);
                }
                b"IDAT" => idat.extend_from_slice(payload),
                b"IEND" => {}
                other => panic!("unexpected chunk {other:?}"),
            }
            pos += 12 + len;
        }
        assert_eq!(dims, (w, h));

        let filtered = zlib_inflate(&idat);
        // Undo the Up filter and compare with the original pixels.
        let stride = w as usize * 3;
        let mut decoded = vec![0u8; stride * h as usize];
        for row in 0..h as usize {
            let f = &filtered[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
            assert_eq!(filtered[row * (stride + 1)], 2, "row filter type");
            if row == 0 {
                decoded[..stride].copy_from_slice(f);
            } else {
                let (prev, cur) = decoded.split_at_mut(row * stride);
                let prev = &prev[(row - 1) * stride..];
                for (i, b) in f.iter().enumerate() {
                    cur[i] = b.wrapping_add(prev[i]);
                }
            }
        }
        assert_eq!(decoded, frame);
    }

    #[test]
    fn png_compression_is_effective_on_game_like_content() {
        // Flat-ish content should compress far below raw size.
        let w = 480u32;
        let h = 270u32;
        let mut frame = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            let shade = (y / 8) as u8 * 6;
            for _ in 0..w {
                frame.extend_from_slice(&[shade, shade, (shade / 2) + 40]);
            }
        }
        let png = encode_png_rgb(w, h, &frame);
        assert!(
            png.len() < frame.len() / 10,
            "flat content should compress well: {} bytes for {} raw",
            png.len(),
            frame.len()
        );
    }
}
#[cfg(test)]
mod debug3 {
    use super::tests::{zlib_inflate, zlib_wrap};
    use super::*;

    #[test]
    fn bisect() {
        // pure literals
        let d1 = b"abcdefghij".to_vec();
        let compressed = zlib_wrap(&d1, &deflate_fixed(&d1));
        let r1 = zlib_inflate(&compressed);
        println!("literals: {} {:?}", r1.len() == d1.len(), r1 == d1);
        if r1 != d1 { println!("  got {:?} want {:?}", r1, d1); }

        // single small match
        let d2 = b"abcabcabcabc".to_vec();
        let r2 = zlib_inflate(&zlib_wrap(&d2, &deflate_fixed(&d2)));
        println!("match: {} got {:?} want {:?}", r2 == d2, r2, d2);

        // single byte
        let d3 = b"a".to_vec();
        let r3 = zlib_inflate(&zlib_wrap(&d3, &deflate_fixed(&d3)));
        println!("one byte: {:?} == {:?}", r3, d3);
    }
}
