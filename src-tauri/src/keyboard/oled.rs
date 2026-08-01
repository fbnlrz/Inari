//! The Apex keyboards' 128x40 OLED.
//!
//! Two things vary between boards, and they vary independently: how a frame is
//! addressed (one report, or eight offset-addressed chunks) and how the pixels
//! are packed (row major, or SSD1306 page major). [`OledWire`] is the product
//! of the two.
//!
//! **Verified on an Apex Pro TKL Wireless (2023)** on 2026-08-01: eight chunks
//! of 641 bytes with command `0x0C`, carrying **row-major** pixels. Both were
//! settled by experiment — vertical bars came out right under either packing,
//! so a top-half-lit frame was used to tell them apart, and it came back as a
//! clean top half. `apex-tux` uses page-major for the *Gen 3* wireless boards,
//! so both packings are kept and the model table picks.
//!
//! One frame is enough: content written this way stayed on the panel minutes
//! later. The firmware does repaint its own screen on its own events, so the
//! draw loop refreshes anyway.

use crate::headset::oled::{Framebuffer, WIDTH};

use super::protocol::OledWire;

/// Panel height. Half the base station's, which is why the keyboard has its
/// own renderers rather than reusing the headset's.
pub const HEIGHT: usize = 40;

/// Bytes of pixel data in one frame: 128 x 40 at 1bpp.
const FRAME_BYTES: usize = WIDTH * HEIGHT / 8;
/// Sub-command inside a chunked write.
const CHUNK_SUBCMD: u8 = 0x01;
const CHUNK_SIZE: usize = 80;

/// A blank framebuffer of the right size for this panel.
pub fn framebuffer() -> Framebuffer {
    Framebuffer::with_height(HEIGHT)
}

/// The framebuffer as the panel's own memory layout: five pages of 128
/// columns, one byte per column with eight stacked pixels, bit 0 topmost.
fn page_major(fb: &Framebuffer) -> [u8; FRAME_BYTES] {
    let mut out = [0u8; FRAME_BYTES];
    // A framebuffer built for the taller base-station panel would run past the
    // end of this one, so encode only the rows both have.
    for y in 0..HEIGHT.min(fb.height()) {
        let page = y / 8;
        let mask = 1u8 << (y % 8);
        for x in 0..WIDTH {
            if fb.get(x, y) {
                out[page * WIDTH + x] |= mask;
            }
        }
    }
    out
}

/// The framebuffer row by row, MSB first — the 2019 panels' layout.
fn row_major(fb: &Framebuffer) -> [u8; FRAME_BYTES] {
    let mut out = [0u8; FRAME_BYTES];
    for y in 0..HEIGHT.min(fb.height()) {
        for x in 0..WIDTH {
            if fb.get(x, y) {
                out[y * (WIDTH / 8) + (x >> 3)] |= 0x80 >> (x & 7);
            }
        }
    }
    out
}

/// The four-byte header OmniLED prefixes on the wired Apex Pro TKL Gen 3.
const GEN3_WIRED_PREFIX: [u8; 4] = [0x38, 0x83, 0x00, 0x00];

/// Encode one frame into the feature reports this transport expects.
/// `feature_len` is the model's declared report size — the hardware is strict
/// about it, and a report shorter than the payload is never produced.
pub fn frame_packets(fb: &Framebuffer, wire: OledWire, feature_len: usize) -> Vec<Vec<u8>> {
    let bitmap = |page: bool| if page { page_major(fb) } else { row_major(fb) };
    match wire {
        OledWire::Single { cmd, page } => {
            vec![single_report(&[cmd], &bitmap(page), feature_len)]
        }
        OledWire::Gen3Wired => vec![single_report(
            &GEN3_WIRED_PREFIX,
            &page_major(fb),
            feature_len,
        )],
        OledWire::Chunked { cmd, page } => chunked(&bitmap(page), cmd, feature_len),
    }
}

fn single_report(prefix: &[u8], data: &[u8; FRAME_BYTES], feature_len: usize) -> Vec<u8> {
    let mut report = vec![0u8; feature_len.max(prefix.len() + FRAME_BYTES)];
    report[..prefix.len()].copy_from_slice(prefix);
    report[prefix.len()..prefix.len() + FRAME_BYTES].copy_from_slice(data);
    report
}

fn chunked(data: &[u8; FRAME_BYTES], cmd: u8, feature_len: usize) -> Vec<Vec<u8>> {
    (0..FRAME_BYTES / CHUNK_SIZE)
        .map(|i| {
            let offset = (i * CHUNK_SIZE) as u16;
            let mut report = vec![0u8; feature_len.max(6 + CHUNK_SIZE)];
            report[0] = cmd;
            report[1] = CHUNK_SUBCMD;
            report[2..4].copy_from_slice(&offset.to_le_bytes());
            report[4] = CHUNK_SIZE as u8;
            report[6..6 + CHUNK_SIZE].copy_from_slice(&data[i * CHUNK_SIZE..(i + 1) * CHUNK_SIZE]);
            report
        })
        .collect()
}

/// Transports to try, best guess first.
///
/// A rejected report is visible (the ioctl fails); a wrong *packing* is not —
/// the panel takes it and shows noise. So the manager walks this list until a
/// write is accepted, and the UI lets the user pin one when a board accepts a
/// frame and still looks scrambled.
pub fn candidates(preferred: OledWire) -> Vec<OledWire> {
    let mut out = vec![preferred];
    out.extend(
        ALL_WIRES
            .iter()
            .map(|(w, _)| *w)
            .filter(|w| *w != preferred),
    );
    out
}

/// Every transport the UI offers, with a label. Ordered by how likely each is
/// to be the right one on an unknown board: the verified shape first, then the
/// ones OmniLED and apex-tux ship working configs for.
pub const ALL_WIRES: [(OledWire, &str); 8] = [
    (
        OledWire::Chunked {
            cmd: 0x0c,
            page: false,
        },
        "Chunked 0x0C, row-major (2023 wireless, verified)",
    ),
    (
        OledWire::Chunked {
            cmd: 0x4c,
            page: false,
        },
        "Chunked 0x4C, row-major (2023 dongle)",
    ),
    (
        OledWire::Single {
            cmd: 0x0a,
            page: true,
        },
        "Single 0x0A, page-major (Gen 3 wireless, cable)",
    ),
    (
        OledWire::Single {
            cmd: 0x4a,
            page: true,
        },
        "Single 0x4A, page-major (Gen 3 wireless, dongle)",
    ),
    (
        OledWire::Gen3Wired,
        "Single 0x38 0x83, page-major (Gen 3 TKL wired)",
    ),
    (
        OledWire::Single {
            cmd: 0x61,
            page: true,
        },
        "Single 0x61, page-major (Apex Pro Gen 3)",
    ),
    (
        OledWire::Single {
            cmd: 0x61,
            page: false,
        },
        "Single 0x61, row-major (Apex Pro / 7 / 5)",
    ),
    (
        OledWire::Chunked {
            cmd: 0x0c,
            page: true,
        },
        "Chunked 0x0C, page-major",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The size the verified board declares.
    const LEN: usize = 641;

    #[test]
    fn a_taller_framebuffer_is_cropped_instead_of_overrunning() {
        // The headset's renderers make 128x64 frames; feeding one here must
        // not panic or bleed rows into the next page.
        let mut tall = Framebuffer::new();
        tall.fill_rect(0, 0, WIDTH, 64, true);
        let packets = frame_packets(
            &tall,
            OledWire::Single {
                cmd: 0x61,
                page: false,
            },
            642,
        );
        assert_eq!(packets.len(), 1);
        assert!(packets[0][1..=FRAME_BYTES].iter().all(|b| *b == 0xff));
    }

    #[test]
    fn the_panel_is_five_pages_tall() {
        assert_eq!(HEIGHT % 8, 0);
        assert_eq!(FRAME_BYTES, 640);
        assert_eq!(framebuffer().height(), HEIGHT);
    }

    #[test]
    fn single_reports_lead_with_their_command_and_fill_the_declared_size() {
        let fb = framebuffer();
        for page in [true, false] {
            let packets = frame_packets(&fb, OledWire::Single { cmd: 0x61, page }, 642);
            assert_eq!(packets.len(), 1);
            assert_eq!(packets[0].len(), 642);
            assert_eq!(packets[0][0], 0x61);
            assert!(packets[0][1..].iter().all(|b| *b == 0), "blank frame");
        }
    }

    #[test]
    fn the_wired_gen3_header_is_four_bytes_and_stretches_the_report() {
        let packets = frame_packets(&framebuffer(), OledWire::Gen3Wired, 642);
        assert_eq!(packets.len(), 1);
        // Prefix plus a full bitmap is longer than the nominal report, and the
        // payload must never be truncated to fit.
        assert_eq!(packets[0].len(), 644);
        assert_eq!(&packets[0][..4], &[0x38, 0x83, 0x00, 0x00]);
    }

    #[test]
    fn the_top_left_pixel_lands_where_each_packing_expects_it() {
        let mut fb = framebuffer();
        fb.set(0, 0, true);
        // Row major: leftmost pixel of the first row is the high bit.
        let row = OledWire::Single {
            cmd: 0x61,
            page: false,
        };
        let page = OledWire::Single {
            cmd: 0x61,
            page: true,
        };
        assert_eq!(frame_packets(&fb, row, 642)[0][1], 0x80);
        // Page major: first column of page 0, topmost pixel is bit 0.
        assert_eq!(frame_packets(&fb, page, 642)[0][1], 0x01);
    }

    #[test]
    fn page_major_stacks_eight_rows_into_one_column_byte() {
        let mut fb = framebuffer();
        for y in 0..8 {
            fb.set(5, y, true);
        }
        fb.set(5, 8, true); // first row of the next page
        let data = frame_packets(
            &fb,
            OledWire::Single {
                cmd: 0x61,
                page: true,
            },
            642,
        )
        .remove(0);
        assert_eq!(data[1 + 5], 0xff, "column 5 of page 0 is fully lit");
        assert_eq!(data[1 + 128 + 5], 0x01, "and page 1 has only its top row");
    }

    #[test]
    fn row_major_keeps_rows_16_bytes_apart() {
        let mut fb = framebuffer();
        fb.set(0, 1, true);
        let data = frame_packets(
            &fb,
            OledWire::Single {
                cmd: 0x61,
                page: false,
            },
            642,
        )
        .remove(0);
        assert_eq!(data[1 + 16], 0x80);
        assert_eq!(data[1], 0x00);
    }

    #[test]
    fn chunked_writes_cover_the_frame_exactly_once() {
        let mut fb = framebuffer();
        fb.set(127, 39, true); // bottom right: the very last byte of the buffer
        let packets = frame_packets(
            &fb,
            OledWire::Chunked {
                cmd: 0x0c,
                page: false,
            },
            LEN,
        );
        assert_eq!(packets.len(), 8);
        let offsets: Vec<u16> = packets
            .iter()
            .map(|p| u16::from_le_bytes([p[2], p[3]]))
            .collect();
        assert_eq!(offsets, vec![0, 80, 160, 240, 320, 400, 480, 560]);
        for p in &packets {
            assert_eq!(p.len(), LEN, "the hardware refuses any other length");
            assert_eq!(p[0], 0x0c);
            assert_eq!(p[1], 0x01);
            assert_eq!(p[4], 80);
        }
        // Row major: bottom right pixel is row 39, byte 39*16+15 = 639 -> the
        // last byte of the last chunk, high bit.
        assert_eq!(packets[7][6 + 79], 0x01);
    }

    #[test]
    fn the_dongle_and_cable_chunk_commands_differ() {
        let fb = framebuffer();
        let dongle = OledWire::Chunked {
            cmd: 0x4c,
            page: false,
        };
        let cable = OledWire::Chunked {
            cmd: 0x0c,
            page: false,
        };
        assert_eq!(frame_packets(&fb, dongle, LEN)[0][0], 0x4c);
        assert_eq!(frame_packets(&fb, cable, LEN)[0][0], 0x0c);
    }

    #[test]
    fn the_preferred_transport_is_probed_first_and_nothing_repeats() {
        let list = candidates(OledWire::Gen3Wired);
        assert_eq!(list[0], OledWire::Gen3Wired);
        assert_eq!(list.len(), ALL_WIRES.len());
        let mut seen = list.clone();
        seen.sort_by_key(|w| format!("{w:?}"));
        seen.dedup();
        assert_eq!(seen.len(), list.len());
    }

    #[test]
    fn every_offered_transport_can_be_probed() {
        for (wire, label) in ALL_WIRES {
            assert!(!label.is_empty());
            assert!(candidates(wire).contains(&wire));
        }
    }
}
