//! The KTX2 Data Format Descriptor (spec 6.3), derived from hl's
//! `FormatKind` for colour and single-aspect depth/stencil, and taken from
//! Khronos' reference writer (`dfd_ref`) for block-compressed formats.
//! Primaries are always UNSPECIFIED: the capture records no colour space.

use crate::dfd_ref::reference;
use crate::vkformat::VkFormat;
use gputools_replay_hl::format::{
    Component, CompressionScheme, DepthKind, FormatKind, NumericKind, StencilKind, format_kind,
};

/// Byte offset of `primaries` within a DFD (after the 4-byte total size,
/// 4-byte vendor/descriptor ids, 4-byte version/size, and `colorModel`).
pub const PRIMARIES_OFFSET: usize = 13;

const MODEL_RGBSDA: u8 = 1;
const PRIMARIES_UNSPECIFIED: u8 = 0;
const TRANSFER_LINEAR: u8 = 1;
const TRANSFER_SRGB: u8 = 2;
const FLAG_ALPHA_STRAIGHT: u8 = 0;
const CH_R: u8 = 0;
const CH_G: u8 = 1;
const CH_B: u8 = 2;
const CH_STENCIL: u8 = 13;
const CH_DEPTH: u8 = 14;
const CH_A: u8 = 15;
const QUAL_LINEAR: u8 = 0x10;
const QUAL_SIGNED: u8 = 0x40;
const QUAL_FLOAT: u8 = 0x80;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DfdError {
    #[error("packed colour format: channel widths are not byte-aligned")]
    Packed,
    #[error("combined depth-stencil is never served as one image")]
    CombinedDepthStencil,
    #[error("PVRTC has no VkFormat and no KTX2 data format descriptor")]
    Pvrtc,
    #[error("no reference data format descriptor captured for {0}")]
    NoReference(&'static str),
    #[error("format not described by gputools-replay-hl's format table")]
    Unknown,
}

struct Sample {
    bit_offset: u16,
    bits: u8,
    channel: u8,
    qualifiers: u8,
    lower: u32,
    upper: u32,
}

/// `(lower, upper, qualifiers)` per the Khronos Data Format spec, as
/// `ktx create` writes them (MEASURED, tests/fixtures/dfd).
fn range(numeric: NumericKind, bits: u8) -> Result<(u32, u32, u8), DfdError> {
    let max_unsigned = if bits >= 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    };
    let max_signed = if bits >= 32 {
        i32::MAX
    } else {
        (1i32 << (bits - 1)) - 1
    };
    Ok(match numeric {
        NumericKind::Unorm => (0, max_unsigned, 0),
        NumericKind::Snorm => ((-max_signed) as u32, max_signed as u32, QUAL_SIGNED),
        NumericKind::Uint => (0, 1, 0),
        NumericKind::Sint => (u32::MAX, 1, QUAL_SIGNED),
        NumericKind::Float => (0xbf80_0000, 0x3f80_0000, QUAL_SIGNED | QUAL_FLOAT),
        NumericKind::SharedExponent => return Err(DfdError::Packed),
    })
}

fn assemble(samples: &[Sample], bytes_plane0: u8, transfer: u8) -> Vec<u8> {
    let block_size = 24 + 16 * samples.len() as u32;
    let mut d = Vec::with_capacity(4 + block_size as usize);
    d.extend_from_slice(&(block_size + 4).to_le_bytes()); // dfdTotalSize
    d.extend_from_slice(&0u32.to_le_bytes()); // vendorId 0, descriptorType 0
    d.extend_from_slice(&2u16.to_le_bytes()); // versionNumber
    d.extend_from_slice(&(block_size as u16).to_le_bytes()); // descriptorBlockSize
    d.extend_from_slice(&[
        MODEL_RGBSDA,
        PRIMARIES_UNSPECIFIED,
        transfer,
        FLAG_ALPHA_STRAIGHT,
    ]);
    d.extend_from_slice(&[0, 0, 0, 0]); // texelBlockDimension0..3 => 1x1x1x1
    let mut planes = [0u8; 8];
    planes[0] = bytes_plane0;
    d.extend_from_slice(&planes);
    for s in samples {
        d.extend_from_slice(&s.bit_offset.to_le_bytes());
        d.push(s.bits - 1);
        d.push(s.channel | s.qualifiers);
        d.extend_from_slice(&[0, 0, 0, 0]); // samplePosition0..3
        d.extend_from_slice(&s.lower.to_le_bytes());
        d.extend_from_slice(&s.upper.to_le_bytes());
    }
    d
}

/// Build the DFD for Metal format `mtl_raw`, whose KTX2 identity is `vk`.
pub fn build(mtl_raw: u32, vk: &VkFormat) -> Result<Vec<u8>, DfdError> {
    match format_kind(mtl_raw) {
        FormatKind::Color(c) => {
            if !c.byte_aligned {
                return Err(DfdError::Packed);
            }
            let mut samples = Vec::with_capacity(c.channels.len());
            let mut offset: u16 = 0;
            for ch in c.channels {
                let (lower, upper, mut qualifiers) = range(c.numeric, ch.bits)?;
                let channel = match ch.component {
                    Component::R => CH_R,
                    Component::G => CH_G,
                    Component::B => CH_B,
                    Component::A => {
                        if c.srgb {
                            // alpha sample of an sRGB format is linear, bypasses the block's sRGB transfer
                            qualifiers |= QUAL_LINEAR;
                        }
                        CH_A
                    }
                };
                samples.push(Sample {
                    bit_offset: offset,
                    bits: ch.bits,
                    channel,
                    qualifiers,
                    lower,
                    upper,
                });
                offset += u16::from(ch.bits);
            }
            let transfer = if c.srgb {
                TRANSFER_SRGB
            } else {
                TRANSFER_LINEAR
            };
            Ok(assemble(&samples, c.bytes_per_pixel as u8, transfer))
        }
        FormatKind::DepthStencil(d) => match (d.depth, d.stencil) {
            (Some(depth), None) => {
                let (bits, numeric, bpp) = match depth {
                    DepthKind::Unorm16 => (16u8, NumericKind::Unorm, 2u8),
                    DepthKind::Float32 => (32, NumericKind::Float, 4),
                };
                let (lower, upper, qualifiers) = range(numeric, bits)?;
                let s = Sample {
                    bit_offset: 0,
                    bits,
                    channel: CH_DEPTH,
                    qualifiers,
                    lower,
                    upper,
                };
                Ok(assemble(&[s], bpp, TRANSFER_LINEAR))
            }
            (None, Some(StencilKind::Uint8)) => {
                let (lower, upper, qualifiers) = range(NumericKind::Uint, 8)?;
                let s = Sample {
                    bit_offset: 0,
                    bits: 8,
                    channel: CH_STENCIL,
                    qualifiers,
                    lower,
                    upper,
                };
                Ok(assemble(&[s], 1, TRANSFER_LINEAR))
            }
            _ => Err(DfdError::CombinedDepthStencil),
        },
        FormatKind::Compressed(c) => {
            if c.scheme == CompressionScheme::Pvrtc {
                return Err(DfdError::Pvrtc);
            }
            let mut d = reference(vk.name)
                .ok_or(DfdError::NoReference(vk.name))?
                .to_vec();
            if let Some(p) = d.get_mut(PRIMARIES_OFFSET) {
                *p = PRIMARIES_UNSPECIFIED;
            }
            Ok(d)
        }
        FormatKind::Unknown => Err(DfdError::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vkformat::{ROWS, lookup};
    use gputools_replay_hl::MTLPixelFormat;

    fn expected(name: &str) -> Vec<u8> {
        let mut d = reference(name).unwrap().to_vec();
        d[PRIMARIES_OFFSET] = PRIMARIES_UNSPECIFIED;
        d
    }

    #[test]
    fn every_supported_format_reproduces_the_reference_writer_dfd() {
        for (mtl, vk) in ROWS {
            let ours = build(*mtl, vk).unwrap_or_else(|e| panic!("{}: {e}", vk.name));
            assert_eq!(
                ours,
                expected(vk.name),
                "{} (mtl {mtl}) differs from ktx create",
                vk.name
            );
        }
    }

    #[test]
    fn primaries_are_unspecified_and_srgb_sets_the_transfer() {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm_sRGB).unwrap();
        let d = build(81, &vk).unwrap();
        assert_eq!(d[PRIMARIES_OFFSET], 0);
        assert_eq!(d[14], TRANSFER_SRGB);
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        assert_eq!(build(80, &vk).unwrap()[14], TRANSFER_LINEAR);
    }

    #[test]
    fn bgra_samples_are_in_memory_order() {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        let d = build(80, &vk).unwrap();
        // sample i starts at 28 + 16*i; byte 3 of a sample is channelType.
        let chan = |i: usize| d[28 + 16 * i + 3] & 0x0f;
        assert_eq!(
            [chan(0), chan(1), chan(2), chan(3)],
            [CH_B, CH_G, CH_R, CH_A]
        );
    }

    #[test]
    fn excluded_kinds_are_named_errors() {
        let fake = VkFormat {
            name: "NONE",
            value: 0,
            type_size: 1,
        };
        assert_eq!(build(90, &fake), Err(DfdError::Packed)); // RGB10A2Unorm
        assert_eq!(build(93, &fake), Err(DfdError::Packed)); // RGB9E5Float
        assert_eq!(build(160, &fake), Err(DfdError::Pvrtc));
        assert_eq!(build(260, &fake), Err(DfdError::CombinedDepthStencil));
        assert_eq!(build(0xffff_ff00, &fake), Err(DfdError::Unknown));
        assert_eq!(build(204, &fake), Err(DfdError::NoReference("NONE")));
    }

    #[test]
    fn srgb_alpha_has_linear_qualifier() {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm_sRGB).unwrap();
        let d = build(81, &vk).unwrap();
        // sample i starts at 28 + 16*i; byte 3 of a sample is channelType.
        let chan_type = |i: usize| d[28 + 16 * i + 3];
        assert_eq!(
            chan_type(3),
            CH_A | QUAL_LINEAR,
            "alpha has linear qualifier"
        );
        assert_eq!(chan_type(0) & 0xf0, 0, "B sample has no qualifiers");
        assert_eq!(chan_type(1) & 0xf0, 0, "G sample has no qualifiers");
        assert_eq!(chan_type(2) & 0xf0, 0, "R sample has no qualifiers");
    }
}
