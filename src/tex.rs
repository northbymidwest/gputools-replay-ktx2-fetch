//! The seam between hl's `Texture` and this tool: a trait exposing exactly
//! what the sweep and emitter read, so both are unit-tested with a fake
//! (hl's `Texture` cannot be constructed outside hl).

use gputools_replay_hl::format::{DepthKind, FormatKind, StencilKind, format_kind};
use gputools_replay_hl::{Error, MTLPixelFormat, Texture};
use serde::Serialize;
use std::borrow::Cow;

/// Which aspect a fetched image is (spec 5 step 3). `Color` covers every
/// colour and compressed format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Aspect {
    Color,
    Depth,
    Stencil,
}

pub fn classify(kind: &FormatKind) -> Aspect {
    if kind.is_depth_only() {
        Aspect::Depth
    } else if kind.is_stencil_only() {
        Aspect::Stencil
    } else {
        Aspect::Color
    }
}

/// Bytes per pixel as the replayer serves them: a colour format's stride,
/// or a single aspect's own element size (a stencil aspect is 1 byte per
/// pixel whatever the nominal X-padded Metal stride says). `None` for
/// compressed, combined depth-stencil, and unknown formats.
pub fn aspect_bpp(kind: &FormatKind) -> Option<usize> {
    match kind {
        FormatKind::Color(c) => Some(c.bytes_per_pixel),
        FormatKind::DepthStencil(d) => match (d.depth, d.stencil) {
            (Some(DepthKind::Unorm16), None) => Some(2),
            (Some(DepthKind::Float32), None) => Some(4),
            (None, Some(StencilKind::Uint8)) => Some(1),
            _ => None,
        },
        FormatKind::Compressed(_) | FormatKind::Unknown => None,
    }
}

/// The bytes a KTX2 level is written from.
pub enum Payload<'a> {
    /// Tightly packed rows. `Cow::Owned` when padding had to be dropped.
    Pixels(Cow<'a, [u8]>),
    /// Raw compressed blocks at the fetched row stride, and the tight
    /// length hl computes from the geometry.
    Blocks {
        bytes: &'a [u8],
        expected_len: usize,
    },
}

pub trait Tex {
    fn stream_ref(&self) -> u64;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn bytes_per_row(&self) -> u32;
    fn bytes_per_image(&self) -> u32;
    fn format(&self) -> MTLPixelFormat;
    fn format_kind(&self) -> FormatKind {
        format_kind(self.format().0 as u32)
    }
    fn raw_bytes(&self) -> &[u8];
    fn payload(&self) -> Result<Payload<'_>, Error>;
}

impl Tex for Texture {
    fn stream_ref(&self) -> u64 {
        Texture::stream_ref(self)
    }
    fn width(&self) -> u32 {
        Texture::width(self)
    }
    fn height(&self) -> u32 {
        Texture::height(self)
    }
    fn bytes_per_row(&self) -> u32 {
        Texture::bytes_per_row(self)
    }
    fn bytes_per_image(&self) -> u32 {
        Texture::bytes_per_image(self)
    }
    fn format(&self) -> MTLPixelFormat {
        Texture::format(self)
    }
    fn raw_bytes(&self) -> &[u8] {
        Texture::raw_bytes(self)
    }
    fn payload(&self) -> Result<Payload<'_>, Error> {
        match Texture::format_kind(self) {
            FormatKind::Compressed(_) => {
                let b = self.blocks()?;
                Ok(Payload::Blocks {
                    bytes: b.bytes,
                    expected_len: b.expected_len(),
                })
            }
            _ => Ok(Payload::Pixels(self.packed_bytes()?)),
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;

    /// A texture built from parts, mimicking hl's `packed_bytes`/`blocks`
    /// contract closely enough for the emitter and sweep tests.
    #[derive(Debug, Clone)]
    pub struct FakeTex {
        pub stream_ref: u64,
        pub width: u32,
        pub height: u32,
        pub bytes_per_row: u32,
        pub mtl_raw: u32,
        pub bytes: Vec<u8>,
    }

    impl FakeTex {
        pub fn new(
            stream_ref: u64,
            width: u32,
            height: u32,
            bytes_per_row: u32,
            mtl_raw: u32,
            bytes: Vec<u8>,
        ) -> Self {
            Self {
                stream_ref,
                width,
                height,
                bytes_per_row,
                mtl_raw,
                bytes,
            }
        }

        /// An unpadded texture filled with one pixel value.
        pub fn solid(stream_ref: u64, width: u32, height: u32, mtl_raw: u32, pixel: &[u8]) -> Self {
            let bpr = width * pixel.len() as u32;
            let bytes = pixel.repeat((width * height) as usize);
            Self::new(stream_ref, width, height, bpr, mtl_raw, bytes)
        }
    }

    impl Tex for FakeTex {
        fn stream_ref(&self) -> u64 {
            self.stream_ref
        }
        fn width(&self) -> u32 {
            self.width
        }
        fn height(&self) -> u32 {
            self.height
        }
        fn bytes_per_row(&self) -> u32 {
            self.bytes_per_row
        }
        fn bytes_per_image(&self) -> u32 {
            self.bytes_per_row * self.height
        }
        fn format(&self) -> MTLPixelFormat {
            MTLPixelFormat(self.mtl_raw as _)
        }
        fn raw_bytes(&self) -> &[u8] {
            &self.bytes
        }
        fn payload(&self) -> Result<Payload<'_>, Error> {
            let kind = self.format_kind();
            if let FormatKind::Compressed(c) = &kind {
                let cols = (self.width as usize).div_ceil(c.block.0 as usize);
                let rows = (self.height as usize).div_ceil(c.block.1 as usize);
                return Ok(Payload::Blocks {
                    bytes: &self.bytes,
                    expected_len: cols * rows * c.block_bytes as usize,
                });
            }
            let bpp = aspect_bpp(&kind).ok_or(Error::WrongCategory("fake: no per-pixel size"))?;
            let (w, h, bpr) = (
                self.width as usize,
                self.height as usize,
                self.bytes_per_row as usize,
            );
            let row = w * bpp;
            if bpr < row {
                return Err(Error::Truncated);
            }
            if self.bytes.len() < h * bpr {
                return Err(Error::Truncated);
            }
            if bpr == row {
                return Ok(Payload::Pixels(Cow::Borrowed(&self.bytes[..h * bpr])));
            }
            let mut v = Vec::with_capacity(row * h);
            for y in 0..h {
                v.extend_from_slice(&self.bytes[y * bpr..y * bpr + row]);
            }
            Ok(Payload::Pixels(Cow::Owned(v)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeTex;
    use super::*;

    #[test]
    fn classify_by_aspect() {
        assert_eq!(classify(&format_kind(80)), Aspect::Color); // BGRA8Unorm
        assert_eq!(classify(&format_kind(204)), Aspect::Color); // ASTC_4x4_LDR
        assert_eq!(classify(&format_kind(252)), Aspect::Depth); // Depth32Float
        assert_eq!(classify(&format_kind(253)), Aspect::Stencil); // Stencil8
        assert_eq!(classify(&format_kind(261)), Aspect::Stencil); // X32_Stencil8
    }

    #[test]
    fn aspect_bpp_uses_the_served_size_not_the_nominal_stride() {
        assert_eq!(aspect_bpp(&format_kind(261)), Some(1)); // X32_Stencil8 served at 1 B/px
        assert_eq!(aspect_bpp(&format_kind(252)), Some(4));
        assert_eq!(aspect_bpp(&format_kind(250)), Some(2));
        assert_eq!(aspect_bpp(&format_kind(125)), Some(16));
        assert_eq!(aspect_bpp(&format_kind(260)), None);
        assert_eq!(aspect_bpp(&format_kind(204)), None);
    }

    #[test]
    fn fake_payload_borrows_when_tight_and_repacks_when_padded() {
        let tight = FakeTex::solid(1, 2, 2, 80, &[1, 2, 3, 4]);
        assert!(matches!(
            tight.payload().unwrap(),
            Payload::Pixels(Cow::Borrowed(_))
        ));
        let mut bytes = vec![0xEE; 12 * 2];
        for y in 0..2 {
            for x in 0..2 {
                bytes[y * 12 + x * 4..y * 12 + x * 4 + 4]
                    .copy_from_slice(&[x as u8, y as u8, 9, 9]);
            }
        }
        let padded = FakeTex::new(1, 2, 2, 12, 80, bytes);
        match padded.payload().unwrap() {
            Payload::Pixels(Cow::Owned(v)) => {
                assert_eq!(v, vec![0, 0, 9, 9, 1, 0, 9, 9, 0, 1, 9, 9, 1, 1, 9, 9])
            }
            _ => panic!("expected an owned repack"),
        }
        let short = FakeTex::new(1, 4, 4, 16, 70, vec![0; 8]);
        assert!(matches!(short.payload(), Err(Error::Truncated)));
        // Test short stride: bytes_per_row < row_len
        let short_stride = FakeTex::new(1, 2, 2, 4, 80, vec![0; 8]);
        assert!(matches!(short_stride.payload(), Err(Error::Truncated)));
    }
}
