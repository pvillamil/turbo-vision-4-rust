// (C) 2026 - Enzo Lombardi

//! Screenshot rendering: turn the terminal cell buffer into a PNG image.
//!
//! This module renders the in-memory screen buffer (a grid of [`Cell`]s, each
//! with a character and foreground/background colors) into a true-color PNG.
//! Text glyphs are drawn from an embedded 8x16 bitmap font; the box-drawing,
//! block and shade characters used by Turbo Vision frames are rendered
//! procedurally so they stay crisp at any cell size.
//!
//! The PNG encoder is fully self-contained (no external crates): it emits a
//! valid RGB PNG using uncompressed ("stored") DEFLATE blocks, so the produced
//! files are a little larger than a compressed encoder would make them but are
//! readable by every PNG viewer.
//!
//! # Example
//!
//! ```no_run
//! use turbo_vision::terminal::Terminal;
//!
//! let terminal = Terminal::init().unwrap();
//! // ... draw some UI ...
//! terminal.save_screenshot_png("screenshot.png").unwrap();
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::trivially_copy_pass_by_ref,
    reason = "Byte/pixel math in a self-contained PNG encoder narrows widths deliberately, and fixed-size chunk tags are passed by reference for call-site clarity."
)]

use super::draw::Cell;
use super::palette::Attr;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Embedded 8x16 bitmap font covering printable ASCII (`0x20..=0x7E`).
///
/// Layout: 16 bytes per glyph, one byte per scan-line, the most-significant
/// bit being the left-most pixel. This is the `Spleen` 8x16 bitmap font
/// (BSD-2-Clause) by Frederic Cambus — a font designed natively at 8x16, so the
/// glyphs are pixel-perfect with no rasterization. See `fonts/Spleen-LICENSE`.
static FONT_8X16: &[u8] = include_bytes!("font8x16.bin");

/// Glyph cell width of the embedded font, in pixels.
const FONT_W: usize = 8;
/// Glyph cell height of the embedded font, in pixels.
const FONT_H: usize = 16;
/// First codepoint present in [`FONT_8X16`].
const FONT_FIRST: u32 = 0x20;
/// Last codepoint present in [`FONT_8X16`].
const FONT_LAST: u32 = 0x7E;

/// A single glyph rendered as 16 rows of 8 horizontal pixels (bit 7 = left).
type GlyphMask = [u8; FONT_H];

/// Glyph height in pixels, exposed so callers can derive an integer scale.
pub const GLYPH_HEIGHT: usize = FONT_H;
/// Glyph width in pixels.
pub const GLYPH_WIDTH: usize = FONT_W;

/// Default cell used for buffer positions that are missing (out of range).
fn blank_cell() -> Cell {
    Cell::new(' ', Attr::from_u8(0x07))
}

/// Render the screen buffer to a PNG file at `path`.
///
/// * `buffer` - row-major grid of cells (`buffer[y][x]`).
/// * `cols` / `rows` - logical size of the screen in character cells.
/// * `scale` - integer magnification (clamped to at least 1). Each cell is
///   drawn as `GLYPH_WIDTH*scale` x `GLYPH_HEIGHT*scale` pixels. Using an
///   *integer* factor keeps glyphs crisp and preserves both the font's
///   proportions and the tiling of box-drawing characters (non-uniform scaling
///   would distort glyph shapes and inter-character spacing).
///
/// # Errors
///
/// Returns an error if the file cannot be created or written.
pub fn render_to_png(
    buffer: &[Vec<Cell>],
    cols: usize,
    rows: usize,
    scale: usize,
    path: &Path,
) -> io::Result<()> {
    let scale = scale.max(1);
    let cell_w = FONT_W * scale;
    let cell_h = FONT_H * scale;
    let img_w = cols * cell_w;
    let img_h = rows * cell_h;
    let mut rgb = vec![0u8; img_w * img_h * 3];

    for ry in 0..rows {
        let row = buffer.get(ry);
        for cx in 0..cols {
            let cell = row
                .and_then(|r| r.get(cx))
                .copied()
                .unwrap_or_else(blank_cell);
            let (fr, fg, fb) = cell.attr.fg.to_rgb();
            let (br, bg, bb) = cell.attr.bg.to_rgb();
            let mask = glyph_mask(cell.ch);

            for (gy, &row_bits) in mask.iter().enumerate() {
                for gx in 0..FONT_W {
                    let on = (row_bits >> (7 - gx)) & 1 != 0;
                    let (r, g, b) = if on { (fr, fg, fb) } else { (br, bg, bb) };
                    // Emit a scale x scale block of this source pixel.
                    let base_x = (cx * FONT_W + gx) * scale;
                    let base_y = (ry * FONT_H + gy) * scale;
                    for sy in 0..scale {
                        let oy = base_y + sy;
                        for sx in 0..scale {
                            let ox = base_x + sx;
                            let idx = (oy * img_w + ox) * 3;
                            rgb[idx] = r;
                            rgb[idx + 1] = g;
                            rgb[idx + 2] = b;
                        }
                    }
                }
            }
        }
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write_png(&mut writer, img_w, img_h, &rgb)?;
    writer.flush()
}

/// Build the 8x16 pixel mask for a character.
///
/// Printable ASCII comes from the embedded font; the CP437 box-drawing, block,
/// shade and arrow glyphs used by Turbo Vision are drawn procedurally. Anything
/// else falls back to a blank cell (whitespace/control) or `?` (other glyphs).
fn glyph_mask(ch: char) -> GlyphMask {
    let cp = ch as u32;

    if (FONT_FIRST..=FONT_LAST).contains(&cp) {
        return font_glyph(cp);
    }

    if let Some(mask) = procedural_glyph(ch) {
        return mask;
    }

    if ch == '\0' || ch.is_whitespace() || ch.is_control() {
        [0u8; FONT_H]
    } else {
        // Unknown printable glyph: show a question mark so content stays visible.
        font_glyph('?' as u32)
    }
}

/// Copy a glyph out of the embedded font. `cp` must be in `FONT_FIRST..=FONT_LAST`.
fn font_glyph(cp: u32) -> GlyphMask {
    let off = (cp - FONT_FIRST) as usize * FONT_H;
    let mut mask = [0u8; FONT_H];
    mask.copy_from_slice(&FONT_8X16[off..off + FONT_H]);
    mask
}

// ----------------------------------------------------------------------------
// Procedural CP437 glyphs (box drawing, blocks, shades, arrows)
// ----------------------------------------------------------------------------

/// Presence of a single-line stub in one of the four directions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Line {
    None,
    Single,
}

/// Set pixel (x, y) in a mask (no-op if out of the 8x16 bounds).
fn set_px(mask: &mut GlyphMask, x: usize, y: usize) {
    if x < FONT_W && y < FONT_H {
        mask[y] |= 1 << (7 - x);
    }
}

/// Fill a rectangular pixel region `[x0, x1) x [y0, y1)`.
fn fill_rect(mask: &mut GlyphMask, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1 {
        for x in x0..x1 {
            set_px(mask, x, y);
        }
    }
}

// Single-line rail position (centered): columns 3-4, rows 7-8.
const VS0: usize = 3; // vertical single, first col (exclusive end VS0+2)
const HS0: usize = 7; // horizontal single, first row

/// Draw a single-line box-drawing glyph from its four directional stubs.
///
/// Each present stub runs from the cell edge to the centered rail and tiles with
/// the neighboring cell, so corners and tees join cleanly.
fn box_glyph(up: Line, down: Line, left: Line, right: Line) -> GlyphMask {
    let mut mask = [0u8; FONT_H];
    let v0 = VS0;
    let h0 = HS0;
    if up == Line::Single {
        fill_rect(&mut mask, v0, 0, v0 + 2, h0 + 2);
    }
    if down == Line::Single {
        fill_rect(&mut mask, v0, h0, v0 + 2, FONT_H);
    }
    if left == Line::Single {
        fill_rect(&mut mask, 0, h0, v0 + 2, h0 + 2);
    }
    if right == Line::Single {
        fill_rect(&mut mask, v0, h0, FONT_W, h0 + 2);
    }
    mask
}

// Double-line rails (8x16). The two parallel rails straddle the single-line
// center: vertical rails occupy cols 1-2 (left, "VL") and 5-6 (right, "VR");
// horizontal rails occupy rows 5-6 (top, "HT") and 9-10 (bottom, "HB"). Corners
// are built so the OUTER rail bends into the OUTER rail and the INNER into the
// INNER, matching CP437 double-line joinery.
fn double_box(ch: char) -> Option<GlyphMask> {
    // (x0, y0, x1, y1) rectangles for each glyph.
    let rects: &[(usize, usize, usize, usize)] = match ch {
        '═' => &[(0, 5, 8, 7), (0, 9, 8, 11)],
        '║' => &[(1, 0, 3, 16), (5, 0, 7, 16)],
        '╔' => &[(1, 5, 3, 16), (5, 9, 7, 16), (1, 5, 8, 7), (5, 9, 8, 11)],
        '╗' => &[(5, 5, 7, 16), (1, 9, 3, 16), (0, 5, 7, 7), (0, 9, 3, 11)],
        '╚' => &[(1, 0, 3, 11), (5, 0, 7, 7), (1, 9, 8, 11), (5, 5, 8, 7)],
        '╝' => &[(5, 0, 7, 11), (1, 0, 3, 7), (0, 9, 7, 11), (0, 5, 3, 7)],
        '╠' => &[(1, 0, 3, 16), (5, 0, 7, 16), (5, 5, 8, 7), (5, 9, 8, 11)],
        '╣' => &[(1, 0, 3, 16), (5, 0, 7, 16), (0, 5, 3, 7), (0, 9, 3, 11)],
        '╦' => &[
            (0, 5, 8, 7),
            (0, 9, 3, 11),
            (5, 9, 8, 11),
            (1, 9, 3, 16),
            (5, 9, 7, 16),
        ],
        '╩' => &[
            (0, 9, 8, 11),
            (0, 5, 3, 7),
            (5, 5, 8, 7),
            (1, 0, 3, 7),
            (5, 0, 7, 7),
        ],
        '╬' => &[
            (1, 0, 3, 7),
            (1, 9, 3, 16),
            (5, 0, 7, 7),
            (5, 9, 7, 16),
            (0, 5, 3, 7),
            (5, 5, 8, 7),
            (0, 9, 3, 11),
            (5, 9, 8, 11),
        ],
        _ => return None,
    };
    let mut mask = [0u8; FONT_H];
    for &(x0, y0, x1, y1) in rects {
        fill_rect(&mut mask, x0, y0, x1, y1);
    }
    Some(mask)
}

/// Return a procedurally-rendered glyph for the CP437 graphics characters that
/// Turbo Vision uses, or `None` if the character is not one of them.
fn procedural_glyph(ch: char) -> Option<GlyphMask> {
    use Line::{None as N, Single as S};

    // Double-line box drawing has its own corner geometry.
    if let Some(mask) = double_box(ch) {
        return Some(mask);
    }

    let mask = match ch {
        // Single box drawing
        '─' => box_glyph(N, N, S, S),
        '│' => box_glyph(S, S, N, N),
        '┌' => box_glyph(N, S, N, S),
        '┐' => box_glyph(N, S, S, N),
        '└' => box_glyph(S, N, N, S),
        '┘' => box_glyph(S, N, S, N),
        '├' => box_glyph(S, S, N, S),
        '┤' => box_glyph(S, S, S, N),
        '┬' => box_glyph(N, S, S, S),
        '┴' => box_glyph(S, N, S, S),
        '┼' => box_glyph(S, S, S, S),
        // Full / half blocks
        '█' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, 0, 0, FONT_W, FONT_H);
            m
        }
        '▀' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, 0, 0, FONT_W, FONT_H / 2);
            m
        }
        '▄' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, 0, FONT_H / 2, FONT_W, FONT_H);
            m
        }
        '▌' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, 0, 0, FONT_W / 2, FONT_H);
            m
        }
        '▐' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, FONT_W / 2, 0, FONT_W, FONT_H);
            m
        }
        // Shade patterns (dithered)
        '░' => shade(|x, y| x % 2 == 0 && y % 2 == 0),
        '▒' => shade(|x, y| (x + y) % 2 == 0),
        '▓' => shade(|x, y| !(x % 2 == 1 && y % 2 == 1)),
        // Small filled square
        '■' => {
            let mut m = [0u8; FONT_H];
            fill_rect(&mut m, 1, 4, FONT_W - 1, FONT_H - 4);
            m
        }
        // Arrows
        '▲' => triangle(Dir::Up),
        '▼' => triangle(Dir::Down),
        '◄' => triangle(Dir::Left),
        '►' => triangle(Dir::Right),
        // Corner triangles (e.g. window resize handle ◢)
        '◢' => corner_triangle(Corner::LowerRight),
        '◣' => corner_triangle(Corner::LowerLeft),
        '◤' => corner_triangle(Corner::UpperLeft),
        '◥' => corner_triangle(Corner::UpperRight),
        _ => return None,
    };

    Some(mask)
}

/// Which corner a diagonal half-cell triangle points into.
#[derive(Clone, Copy)]
enum Corner {
    LowerRight,
    LowerLeft,
    UpperLeft,
    UpperRight,
}

/// Build a right-triangle filling one diagonal half of the cell.
fn corner_triangle(corner: Corner) -> GlyphMask {
    let (w1, h1) = (FONT_W - 1, FONT_H - 1);
    shade(|x, y| {
        // Normalize coordinates to the diagonal test; pick the half-plane
        // whose right angle sits in the requested corner.
        let (dx, dy) = match corner {
            Corner::LowerRight => (x, h1 - y),
            Corner::LowerLeft => (w1 - x, h1 - y),
            Corner::UpperLeft => (w1 - x, y),
            Corner::UpperRight => (x, y),
        };
        dx * h1 >= dy * w1
    })
}

/// Build a shade glyph from a per-pixel predicate.
fn shade(on: impl Fn(usize, usize) -> bool) -> GlyphMask {
    let mut mask = [0u8; FONT_H];
    for y in 0..FONT_H {
        for x in 0..FONT_W {
            if on(x, y) {
                set_px(&mut mask, x, y);
            }
        }
    }
    mask
}

/// Direction for arrow glyphs.
#[derive(Clone, Copy)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Build a filled triangle pointing in the given direction.
fn triangle(dir: Dir) -> GlyphMask {
    let mut mask = [0u8; FONT_H];
    match dir {
        // Vertical arrows: widen one row per step over the middle of the cell.
        Dir::Up => {
            for (i, y) in (4..12).enumerate() {
                let half = i / 2 + 1;
                let cx = FONT_W / 2;
                fill_rect(
                    &mut mask,
                    cx.saturating_sub(half),
                    y,
                    (cx + half).min(FONT_W),
                    y + 1,
                );
            }
        }
        Dir::Down => {
            for (i, y) in (4..12).enumerate() {
                let half = (8 - i) / 2 + 1;
                let cx = FONT_W / 2;
                fill_rect(
                    &mut mask,
                    cx.saturating_sub(half),
                    y,
                    (cx + half).min(FONT_W),
                    y + 1,
                );
            }
        }
        // Horizontal arrows: widen one column per step.
        Dir::Left => {
            for (i, x) in (1..7).enumerate() {
                let half = i / 2 + 1;
                let cy = FONT_H / 2;
                fill_rect(
                    &mut mask,
                    x,
                    cy.saturating_sub(half),
                    x + 1,
                    (cy + half).min(FONT_H),
                );
            }
        }
        Dir::Right => {
            for (i, x) in (1..7).enumerate() {
                let half = (6 - i) / 2 + 1;
                let cy = FONT_H / 2;
                fill_rect(
                    &mut mask,
                    x,
                    cy.saturating_sub(half),
                    x + 1,
                    (cy + half).min(FONT_H),
                );
            }
        }
    }
    mask
}

// ----------------------------------------------------------------------------
// Minimal self-contained PNG encoder (RGB, 8-bit, stored DEFLATE)
// ----------------------------------------------------------------------------

/// CRC-32 lookup table (IEEE polynomial), computed at compile time.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb88320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// Compute the PNG/zlib CRC-32 of a byte slice.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

/// Compute the Adler-32 checksum used by the zlib wrapper.
fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in bytes {
        a = (a + u32::from(x)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// Write a single PNG chunk: length, type, data, CRC.
fn write_chunk<W: Write>(w: &mut W, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(kind)?;
    w.write_all(data)?;
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    w.write_all(&crc32(&crc_input).to_be_bytes())
}

/// Wrap raw bytes in a zlib stream using only uncompressed (stored) blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    // zlib header: CM=8/CINFO=7 (0x78), FLG chosen so (0x78<<8 | FLG) % 31 == 0.
    let mut out = vec![0x78u8, 0x01];
    let mut i = 0;
    if raw.is_empty() {
        // One empty final stored block.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    while i < raw.len() {
        let n = (raw.len() - i).min(0xffff);
        let is_final = i + n >= raw.len();
        out.push(u8::from(is_final)); // BFINAL bit, BTYPE = 00 (stored)
        let len = n as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&raw[i..i + n]);
        i += n;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Encode `rgb` (width*height*3 bytes) as an 8-bit RGB PNG.
fn write_png<W: Write>(w: &mut W, width: usize, height: usize, rgb: &[u8]) -> io::Result<()> {
    // PNG signature.
    w.write_all(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])?;

    // IHDR
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: truecolor RGB
    ihdr.push(0); // compression: deflate
    ihdr.push(0); // filter: adaptive
    ihdr.push(0); // interlace: none
    write_chunk(w, b"IHDR", &ihdr)?;

    // IDAT: prepend a filter byte (0 = none) to each scanline, then zlib-wrap.
    let stride = width * 3;
    let mut raw = Vec::with_capacity(height * (stride + 1));
    for y in 0..height {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }
    let idat = zlib_stored(&raw);
    write_chunk(w, b"IDAT", &idat)?;

    // IEND
    write_chunk(w, b"IEND", &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::palette::TvColor;

    #[test]
    fn crc32_known_value() {
        // CRC-32 of "IEND" is a well-known constant.
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn adler32_known_value() {
        // Adler-32 of a single zero byte: a=1, b=1.
        assert_eq!(adler32(&[0]), 0x0001_0001);
    }

    #[test]
    fn ascii_glyph_is_nonblank() {
        // 'A' must have some set pixels.
        let m = glyph_mask('A');
        assert!(m.iter().any(|&row| row != 0));
    }

    #[test]
    fn space_is_blank() {
        assert_eq!(glyph_mask(' '), [0u8; FONT_H]);
    }

    #[test]
    fn full_block_is_solid() {
        assert_eq!(glyph_mask('█'), [0xffu8; FONT_H]);
    }

    #[test]
    fn writes_valid_png_header() {
        let buffer = vec![vec![
            Cell::new('H', Attr::new(TvColor::White, TvColor::Blue)),
            Cell::new('i', Attr::new(TvColor::White, TvColor::Blue)),
        ]];
        let dir = std::env::temp_dir();
        let path = dir.join("tv_screenshot_test.png");
        render_to_png(&buffer, 2, 1, 1, &path).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            &bytes[..8],
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
        );
        // IHDR width/height live at byte offset 16..24.
        assert_eq!(&bytes[16..20], &(16u32).to_be_bytes()); // 2 cols * 8px * 1
        assert_eq!(&bytes[20..24], &(16u32).to_be_bytes()); // 1 row * 16px * 1
        let _ = std::fs::remove_file(&path);
    }
}
