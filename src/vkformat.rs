//! The `MTLPixelFormat` -> `VkFormat` table (spec 6.1). Every row is
//! confirmed by `ktx create --raw` + `ktx validate` (the fixtures under
//! `tests/fixtures/dfd/`); a format with no row is a per-texture failure,
//! never a guess. Metal names come from hl's `format::name`.

use gputools_replay_hl::MTLPixelFormat;
use gputools_replay_hl::format::name;

/// One KTX2 format identity: the VkFormat name (as `ktx create` spells it,
/// without the `VK_FORMAT_` prefix), its numeric value, and the KTX2 header
/// `typeSize` (bytes per channel element, 1 for block-compressed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VkFormat {
    pub name: &'static str,
    pub value: u32,
    pub type_size: u32,
}

const fn vk(name: &'static str, value: u32, type_size: u32) -> VkFormat {
    VkFormat {
        name,
        value,
        type_size,
    }
}

/// Every supported row: (raw `MTLPixelFormat` value, KTX2 identity).
pub const ROWS: &[(u32, VkFormat)] = &[
    // -- byte-aligned colour --
    (1, vk("A8_UNORM_KHR", 1000470001, 1)),
    (10, vk("R8_UNORM", 9, 1)),
    (11, vk("R8_SRGB", 15, 1)),
    (12, vk("R8_SNORM", 10, 1)),
    (13, vk("R8_UINT", 13, 1)),
    (14, vk("R8_SINT", 14, 1)),
    (20, vk("R16_UNORM", 70, 2)),
    (22, vk("R16_SNORM", 71, 2)),
    (23, vk("R16_UINT", 74, 2)),
    (24, vk("R16_SINT", 75, 2)),
    (25, vk("R16_SFLOAT", 76, 2)),
    (53, vk("R32_UINT", 98, 4)),
    (54, vk("R32_SINT", 99, 4)),
    (55, vk("R32_SFLOAT", 100, 4)),
    (30, vk("R8G8_UNORM", 16, 1)),
    (31, vk("R8G8_SRGB", 22, 1)),
    (32, vk("R8G8_SNORM", 17, 1)),
    (33, vk("R8G8_UINT", 20, 1)),
    (34, vk("R8G8_SINT", 21, 1)),
    (60, vk("R16G16_UNORM", 77, 2)),
    (62, vk("R16G16_SNORM", 78, 2)),
    (63, vk("R16G16_UINT", 81, 2)),
    (64, vk("R16G16_SINT", 82, 2)),
    (65, vk("R16G16_SFLOAT", 83, 2)),
    (103, vk("R32G32_UINT", 101, 4)),
    (104, vk("R32G32_SINT", 102, 4)),
    (105, vk("R32G32_SFLOAT", 103, 4)),
    (70, vk("R8G8B8A8_UNORM", 37, 1)),
    (71, vk("R8G8B8A8_SRGB", 43, 1)),
    (72, vk("R8G8B8A8_SNORM", 38, 1)),
    (73, vk("R8G8B8A8_UINT", 41, 1)),
    (74, vk("R8G8B8A8_SINT", 42, 1)),
    (80, vk("B8G8R8A8_UNORM", 44, 1)),
    (81, vk("B8G8R8A8_SRGB", 50, 1)),
    (110, vk("R16G16B16A16_UNORM", 91, 2)),
    (112, vk("R16G16B16A16_SNORM", 92, 2)),
    (113, vk("R16G16B16A16_UINT", 95, 2)),
    (114, vk("R16G16B16A16_SINT", 96, 2)),
    (115, vk("R16G16B16A16_SFLOAT", 97, 2)),
    (123, vk("R32G32B32A32_UINT", 107, 4)),
    (124, vk("R32G32B32A32_SINT", 108, 4)),
    (125, vk("R32G32B32A32_SFLOAT", 109, 4)),
    // -- single-aspect depth / stencil (X24/X32 stencil aspects are served
    //    at 1 byte per pixel, hence S8_UINT) --
    (250, vk("D16_UNORM", 124, 2)),
    (252, vk("D32_SFLOAT", 126, 4)),
    (253, vk("S8_UINT", 127, 1)),
    (261, vk("S8_UINT", 127, 1)),
    (262, vk("S8_UINT", 127, 1)),
    // -- BC --
    (130, vk("BC1_RGBA_UNORM_BLOCK", 133, 1)),
    (131, vk("BC1_RGBA_SRGB_BLOCK", 134, 1)),
    (132, vk("BC2_UNORM_BLOCK", 135, 1)),
    (133, vk("BC2_SRGB_BLOCK", 136, 1)),
    (134, vk("BC3_UNORM_BLOCK", 137, 1)),
    (135, vk("BC3_SRGB_BLOCK", 138, 1)),
    (140, vk("BC4_UNORM_BLOCK", 139, 1)),
    (141, vk("BC4_SNORM_BLOCK", 140, 1)),
    (142, vk("BC5_UNORM_BLOCK", 141, 1)),
    (143, vk("BC5_SNORM_BLOCK", 142, 1)),
    (150, vk("BC6H_SFLOAT_BLOCK", 144, 1)),
    (151, vk("BC6H_UFLOAT_BLOCK", 143, 1)),
    (152, vk("BC7_UNORM_BLOCK", 145, 1)),
    (153, vk("BC7_SRGB_BLOCK", 146, 1)),
    // -- EAC / ETC2 --
    (170, vk("EAC_R11_UNORM_BLOCK", 153, 1)),
    (172, vk("EAC_R11_SNORM_BLOCK", 154, 1)),
    (174, vk("EAC_R11G11_UNORM_BLOCK", 155, 1)),
    (176, vk("EAC_R11G11_SNORM_BLOCK", 156, 1)),
    (178, vk("ETC2_R8G8B8A8_UNORM_BLOCK", 151, 1)),
    (179, vk("ETC2_R8G8B8A8_SRGB_BLOCK", 152, 1)),
    (180, vk("ETC2_R8G8B8_UNORM_BLOCK", 147, 1)),
    (181, vk("ETC2_R8G8B8_SRGB_BLOCK", 148, 1)),
    (182, vk("ETC2_R8G8B8A1_UNORM_BLOCK", 149, 1)),
    (183, vk("ETC2_R8G8B8A1_SRGB_BLOCK", 150, 1)),
    // -- ASTC sRGB --
    (186, vk("ASTC_4x4_SRGB_BLOCK", 158, 1)),
    (187, vk("ASTC_5x4_SRGB_BLOCK", 160, 1)),
    (188, vk("ASTC_5x5_SRGB_BLOCK", 162, 1)),
    (189, vk("ASTC_6x5_SRGB_BLOCK", 164, 1)),
    (190, vk("ASTC_6x6_SRGB_BLOCK", 166, 1)),
    (192, vk("ASTC_8x5_SRGB_BLOCK", 168, 1)),
    (193, vk("ASTC_8x6_SRGB_BLOCK", 170, 1)),
    (194, vk("ASTC_8x8_SRGB_BLOCK", 172, 1)),
    (195, vk("ASTC_10x5_SRGB_BLOCK", 174, 1)),
    (196, vk("ASTC_10x6_SRGB_BLOCK", 176, 1)),
    (197, vk("ASTC_10x8_SRGB_BLOCK", 178, 1)),
    (198, vk("ASTC_10x10_SRGB_BLOCK", 180, 1)),
    (199, vk("ASTC_12x10_SRGB_BLOCK", 182, 1)),
    (200, vk("ASTC_12x12_SRGB_BLOCK", 184, 1)),
    // -- ASTC LDR --
    (204, vk("ASTC_4x4_UNORM_BLOCK", 157, 1)),
    (205, vk("ASTC_5x4_UNORM_BLOCK", 159, 1)),
    (206, vk("ASTC_5x5_UNORM_BLOCK", 161, 1)),
    (207, vk("ASTC_6x5_UNORM_BLOCK", 163, 1)),
    (208, vk("ASTC_6x6_UNORM_BLOCK", 165, 1)),
    (210, vk("ASTC_8x5_UNORM_BLOCK", 167, 1)),
    (211, vk("ASTC_8x6_UNORM_BLOCK", 169, 1)),
    (212, vk("ASTC_8x8_UNORM_BLOCK", 171, 1)),
    (213, vk("ASTC_10x5_UNORM_BLOCK", 173, 1)),
    (214, vk("ASTC_10x6_UNORM_BLOCK", 175, 1)),
    (215, vk("ASTC_10x8_UNORM_BLOCK", 177, 1)),
    (216, vk("ASTC_10x10_UNORM_BLOCK", 179, 1)),
    (217, vk("ASTC_12x10_UNORM_BLOCK", 181, 1)),
    (218, vk("ASTC_12x12_UNORM_BLOCK", 183, 1)),
    // -- ASTC HDR --
    (222, vk("ASTC_4x4_SFLOAT_BLOCK", 1000066000, 1)),
    (223, vk("ASTC_5x4_SFLOAT_BLOCK", 1000066001, 1)),
    (224, vk("ASTC_5x5_SFLOAT_BLOCK", 1000066002, 1)),
    (225, vk("ASTC_6x5_SFLOAT_BLOCK", 1000066003, 1)),
    (226, vk("ASTC_6x6_SFLOAT_BLOCK", 1000066004, 1)),
    (228, vk("ASTC_8x5_SFLOAT_BLOCK", 1000066005, 1)),
    (229, vk("ASTC_8x6_SFLOAT_BLOCK", 1000066006, 1)),
    (230, vk("ASTC_8x8_SFLOAT_BLOCK", 1000066007, 1)),
    (231, vk("ASTC_10x5_SFLOAT_BLOCK", 1000066008, 1)),
    (232, vk("ASTC_10x6_SFLOAT_BLOCK", 1000066009, 1)),
    (233, vk("ASTC_10x8_SFLOAT_BLOCK", 1000066010, 1)),
    (234, vk("ASTC_10x10_SFLOAT_BLOCK", 1000066011, 1)),
    (235, vk("ASTC_12x10_SFLOAT_BLOCK", 1000066012, 1)),
    (236, vk("ASTC_12x12_SFLOAT_BLOCK", 1000066013, 1)),
];

/// The KTX2 identity for a Metal format, or `None` when the tool does not
/// write it (packed colour, PVRTC, combined depth-stencil, unknown).
pub fn lookup(fmt: MTLPixelFormat) -> Option<VkFormat> {
    let raw = fmt.0 as u32;
    ROWS.iter().find(|(m, _)| *m == raw).map(|(_, v)| *v)
}

/// hl's canonical Metal name, or `MTLPixelFormat(<raw>)` when hl's table
/// does not describe the value. Used in filenames, KV data, and reasons.
pub fn metal_name(fmt: MTLPixelFormat) -> String {
    match name(fmt) {
        Some(n) => n.to_string(),
        None => format!("MTLPixelFormat({})", fmt.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gputools_replay_hl::format::{CompressionScheme, FormatKind, format_kind};
    use std::collections::HashMap;

    fn index() -> HashMap<String, (u32, u32)> {
        let text = include_str!("../tests/fixtures/dfd/index.tsv");
        text.lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (
                    f[0].to_string(),
                    (f[1].parse().unwrap(), f[2].parse().unwrap()),
                )
            })
            .collect()
    }

    #[test]
    fn every_row_matches_the_reference_writer_header() {
        let idx = index();
        for (mtl, row) in ROWS {
            let (value, type_size) = idx
                .get(row.name)
                .unwrap_or_else(|| panic!("no fixture for {} (mtl {mtl})", row.name));
            assert_eq!(row.value, *value, "{}: vkFormat value", row.name);
            assert_eq!(row.type_size, *type_size, "{}: typeSize", row.name);
        }
    }

    #[test]
    fn every_row_has_an_hl_name_and_a_supported_kind() {
        for (mtl, row) in ROWS {
            assert!(
                name(MTLPixelFormat(*mtl as _)).is_some(),
                "mtl {mtl} ({})",
                row.name
            );
            match format_kind(*mtl) {
                FormatKind::Color(c) => assert!(c.byte_aligned, "mtl {mtl} is packed"),
                FormatKind::DepthStencil(d) => {
                    assert!(
                        d.depth.is_some() != d.stencil.is_some(),
                        "mtl {mtl} is combined"
                    )
                }
                FormatKind::Compressed(c) => {
                    assert_ne!(c.scheme, CompressionScheme::Pvrtc, "mtl {mtl} is PVRTC")
                }
                FormatKind::Unknown => panic!("mtl {mtl} is unknown to hl"),
            }
        }
    }

    #[test]
    fn rows_are_unique_per_metal_format() {
        let mut seen = std::collections::HashSet::new();
        for (mtl, _) in ROWS {
            assert!(seen.insert(*mtl), "duplicate row for mtl {mtl}");
        }
        assert_eq!(ROWS.len(), 113);
    }

    #[test]
    fn excluded_formats_have_no_row() {
        for raw in [
            90u32,
            91,
            92,
            93,
            94,
            40,
            41,
            42,
            43,
            160,
            167,
            260,
            0xffff_ff00,
        ] {
            assert!(
                lookup(MTLPixelFormat(raw as _)).is_none(),
                "raw {raw} must not map"
            );
        }
    }

    #[test]
    fn metal_name_falls_back_to_the_raw_value() {
        assert_eq!(metal_name(MTLPixelFormat::BGRA8Unorm), "BGRA8Unorm");
        assert_eq!(
            metal_name(MTLPixelFormat((0xffff_ff00u32) as _)),
            "MTLPixelFormat(4294967040)"
        );
    }
}
