//! One fetched texture to one `.ktx2` file, or one recorded failure
//! (spec 7). Nothing here aborts a run.

use crate::dfd;
use crate::ktx::{Ktx2Params, write_ktx2};
use crate::manifest::{Attribution, DescriptorEntry, Failure, Manifest, TextureEntry};
use crate::tex::{Aspect, Payload, Tex, aspect_bpp};
use crate::vkformat::{lookup, metal_name};
use gputools_replay_hl::format::FormatKind;
use gputools_replay_hl::{MTLTextureType, TextureDescriptor};
use std::borrow::Cow;
use std::path::Path;

/// A descriptor the join attributed, with the grade from spec 5 step 5.
#[derive(Debug, Clone, Copy)]
pub struct Attributed {
    pub descriptor: TextureDescriptor,
    pub attribution: Attribution,
}

/// One record the sweep decided to write.
pub struct Fetched<T> {
    pub texture: T,
    pub aspect: Aspect,
    /// True for a stencil aspect obtained by the pass-2 probe (plane 1);
    /// such files get the `_stencil` suffix.
    pub probed: bool,
    pub descriptor: Option<Attributed>,
}

pub struct Context<'a> {
    pub out: &'a Path,
    pub bundle: &'a str,
    pub force_load_unused: bool,
}

pub fn texture_type_name(t: MTLTextureType) -> &'static str {
    match t {
        MTLTextureType::Type1D => "1D",
        MTLTextureType::Type1DArray => "1DArray",
        MTLTextureType::Type2D => "2D",
        MTLTextureType::Type2DArray => "2DArray",
        MTLTextureType::Type2DMultisample => "2DMultisample",
        MTLTextureType::TypeCube => "Cube",
        MTLTextureType::TypeCubeArray => "CubeArray",
        MTLTextureType::Type3D => "3D",
        MTLTextureType::Type2DMultisampleArray => "2DMultisampleArray",
        MTLTextureType::TypeTextureBuffer => "TextureBuffer",
        _ => "unknown",
    }
}

fn texture_type(d: &TextureDescriptor) -> MTLTextureType {
    MTLTextureType(d.texture_type as _)
}

pub fn file_name<T: Tex>(f: &Fetched<T>) -> String {
    let mut name = format!(
        "ref{}_{}x{}_{}",
        f.texture.stream_ref(),
        f.texture.width(),
        f.texture.height(),
        metal_name(f.texture.format())
    );
    if f.probed {
        name.push_str("_stencil");
    }
    name.push_str(".ktx2");
    name
}

fn unsupported_reason(kind: &FormatKind) -> &'static str {
    match kind {
        FormatKind::Color(_) => "packed formats are not mapped",
        FormatKind::DepthStencil(_) => "combined depth-stencil is never served as one image",
        FormatKind::Compressed(_) => "PVRTC has no VkFormat",
        FormatKind::Unknown => "not described by gputools-replay-hl's format table",
    }
}

fn provenance_kv<T: Tex>(
    ctx: &Context,
    f: &Fetched<T>,
    rows_repacked: bool,
) -> Vec<(String, String)> {
    let t = &f.texture;
    let aspect = match f.aspect {
        Aspect::Color => "color",
        Aspect::Depth => "depth",
        Aspect::Stencil => "stencil",
    };
    let mut kv = vec![
        (
            "KTXwriter".to_string(),
            format!("gputools-replay-ktx2-fetch {}", crate::TOOL_VERSION),
        ),
        ("gputrace.aspect".to_string(), aspect.to_string()),
        (
            "gputrace.assumptions".to_string(),
            Manifest::assumptions_line(ctx.force_load_unused),
        ),
        ("gputrace.bundle".to_string(), ctx.bundle.to_string()),
        (
            "gputrace.bytesPerImage".to_string(),
            t.bytes_per_image().to_string(),
        ),
        (
            "gputrace.bytesPerRow".to_string(),
            t.bytes_per_row().to_string(),
        ),
        (
            "gputrace.mtlPixelFormat".to_string(),
            format!("{} ({})", metal_name(t.format()), t.format().0),
        ),
        (
            "gputrace.rowsRepacked".to_string(),
            rows_repacked.to_string(),
        ),
        ("gputrace.streamRef".to_string(), t.stream_ref().to_string()),
    ];
    if let Some(a) = &f.descriptor {
        let d = &a.descriptor;
        let grade = match a.attribution {
            Attribution::Certain => "certain",
            Attribution::Ambiguous => "ambiguous",
        };
        kv.push((
            "gputrace.arrayLength".to_string(),
            d.array_length.to_string(),
        ));
        kv.push(("gputrace.depth".to_string(), d.depth.to_string()));
        kv.push((
            "gputrace.descriptorAttribution".to_string(),
            grade.to_string(),
        ));
        kv.push((
            "gputrace.mipLevelCount".to_string(),
            d.mip_levels.to_string(),
        ));
        kv.push((
            "gputrace.textureType".to_string(),
            texture_type_name(texture_type(d)).to_string(),
        ));
        kv.push(("gputrace.textureUsage".to_string(), d.usage.to_string()));
    }
    kv
}

/// Tight compressed block rows: hl's `blocks().bytes` is at the fetched
/// row stride, which may hold padding blocks past `ceil(width / bw)`.
fn packed_blocks<'a>(
    bytes: &'a [u8],
    width: u32,
    height: u32,
    bytes_per_row: u32,
    block: (u8, u8),
    block_bytes: u8,
    expected_len: usize,
) -> Result<Cow<'a, [u8]>, String> {
    let cols = (width as usize).div_ceil(block.0 as usize);
    let rows = (height as usize).div_ceil(block.1 as usize);
    let tight_row = cols * block_bytes as usize;
    let bpr = bytes_per_row as usize;
    if bpr < tight_row {
        return Err(format!(
            "bytesPerRow {bpr} is smaller than the {tight_row} bytes {cols} blocks need"
        ));
    }
    if bytes.len() < rows * bpr {
        return Err(format!(
            "payload is {} bytes but {} block rows of {bpr} bytes need {}",
            bytes.len(),
            rows,
            rows * bpr
        ));
    }
    if bpr == tight_row {
        let got = bytes
            .get(..expected_len)
            .ok_or_else(|| format!("payload shorter than expected {expected_len}"))?;
        return Ok(Cow::Borrowed(got));
    }
    let mut v = Vec::with_capacity(expected_len);
    for r in 0..rows {
        let row = bytes
            .get(r * bpr..r * bpr + tight_row)
            .ok_or_else(|| "block row out of range".to_string())?;
        v.extend_from_slice(row);
    }
    Ok(Cow::Owned(v))
}

/// Write one texture. Every failure is recorded against `(stream_ref,
/// aspect)` in `man.failures`; every success in `man.textures`.
pub fn emit_one<T: Tex>(ctx: &Context, f: &Fetched<T>, man: &mut Manifest) {
    let t = &f.texture;
    let fail = |man: &mut Manifest, reason: String| {
        man.failures.push(Failure {
            stream_ref: t.stream_ref(),
            aspect: f.aspect,
            reason,
        });
    };

    // Spec 5 step 6: a certain-attributed volume with depth > 1 cannot say
    // which z-plane it holds. Depth-1 volumes and ambiguous ones are written.
    if let Some(a) = &f.descriptor
        && a.attribution == Attribution::Certain
        && texture_type(&a.descriptor) == MTLTextureType::Type3D
        && a.descriptor.depth > 1
    {
        return fail(
            man,
            format!(
                "3D texture, depth {}: the fetch serves one unidentified z-plane",
                a.descriptor.depth
            ),
        );
    }

    let kind = t.format_kind();
    let Some(vk) = lookup(t.format()) else {
        return fail(
            man,
            format!(
                "unsupported MTLPixelFormat {} ({}): {}",
                metal_name(t.format()),
                t.format().0,
                unsupported_reason(&kind)
            ),
        );
    };
    let dfd = match dfd::build(t.format().0 as u32, &vk) {
        Ok(d) => d,
        Err(e) => return fail(man, format!("data format descriptor: {e}")),
    };

    let payload = match t.payload() {
        Ok(p) => p,
        Err(e) => return fail(man, format!("payload: {e}")),
    };
    let (bytes, rows_repacked, texel_block_bytes): (Cow<[u8]>, bool, u32) = match (payload, &kind) {
        (Payload::Pixels(p), kind) => {
            let Some(bpp) = aspect_bpp(kind) else {
                return fail(man, "no per-pixel size for this format".to_string());
            };
            let expected = t.width() as usize * t.height() as usize * bpp;
            if p.len() != expected {
                return fail(
                    man,
                    format!(
                        "packed payload is {} bytes but {}x{} at {bpp} B/px needs {expected}",
                        p.len(),
                        t.width(),
                        t.height()
                    ),
                );
            }
            let repacked = matches!(p, Cow::Owned(_));
            (p, repacked, bpp as u32)
        }
        (
            Payload::Blocks {
                bytes,
                expected_len,
            },
            FormatKind::Compressed(c),
        ) => {
            match packed_blocks(
                bytes,
                t.width(),
                t.height(),
                t.bytes_per_row(),
                c.block,
                c.block_bytes,
                expected_len,
            ) {
                Ok(b) => {
                    let repacked = matches!(b, Cow::Owned(_));
                    (b, repacked, u32::from(c.block_bytes))
                }
                Err(e) => return fail(man, format!("compressed payload: {e}")),
            }
        }
        (Payload::Blocks { .. }, _) => {
            return fail(
                man,
                "blocks payload for a non-compressed format".to_string(),
            );
        }
    };

    let kv = provenance_kv(ctx, f, rows_repacked);
    let params = Ktx2Params {
        vk_format: vk.value,
        type_size: vk.type_size,
        width: t.width(),
        height: t.height(),
        texel_block_bytes,
        dfd: &dfd,
        kv: &kv,
    };
    let file = match write_ktx2(&params, &bytes) {
        Ok(b) => b,
        Err(e) => return fail(man, format!("ktx2: {e}")),
    };
    let name = file_name(f);
    if let Err(e) = std::fs::write(ctx.out.join(&name), &file) {
        return fail(man, format!("writing {name}: {e}"));
    }
    man.textures.push(TextureEntry {
        stream_ref: t.stream_ref(),
        aspect: f.aspect,
        file: name,
        mtl_pixel_format: metal_name(t.format()),
        mtl_pixel_format_raw: t.format().0 as u32,
        vk_format: vk.name.to_string(),
        width: t.width(),
        height: t.height(),
        bytes_per_row: t.bytes_per_row(),
        rows_repacked,
        descriptor: f.descriptor.as_ref().map(|a| DescriptorEntry {
            mip_levels: a.descriptor.mip_levels,
            array_length: a.descriptor.array_length,
            depth: a.descriptor.depth,
            texture_type: texture_type_name(texture_type(&a.descriptor)).to_string(),
            usage: a.descriptor.usage,
            attribution: a.attribution,
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ktx::{kv_pairs, level0, parse_header};
    use crate::tex::fake::FakeTex;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gputools_replay_ktx2_fetch_emit_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn desc(texture_type: u32, depth: u32, mips: u32, arr: u32) -> TextureDescriptor {
        TextureDescriptor {
            store0_offset: 0,
            format: 80,
            texture_type,
            width: 4,
            height: 4,
            depth,
            mip_levels: mips,
            array_length: arr,
            sample_count: 1,
            usage: 5,
            texture_id: 0,
        }
    }

    fn run(name: &str, f: &Fetched<FakeTex>) -> (Manifest, PathBuf) {
        let out = scratch(name);
        let ctx = Context {
            out: &out,
            bundle: "cap.gputrace",
            force_load_unused: false,
        };
        let mut man = Manifest::new("cap.gputrace".into(), 10, false, 60);
        emit_one(&ctx, f, &mut man);
        (man, out)
    }

    fn plain(t: FakeTex, aspect: Aspect) -> Fetched<FakeTex> {
        Fetched {
            texture: t,
            aspect,
            probed: false,
            descriptor: None,
        }
    }

    #[test]
    fn unpadded_bgra_passes_through_untouched() {
        let px: Vec<u8> = (0..64).collect();
        let t = FakeTex::new(25, 4, 4, 16, 80, px.clone());
        let (man, out) = run("plain", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        let e = &man.textures[0];
        assert_eq!(e.file, "ref25_4x4_BGRA8Unorm.ktx2");
        assert!(!e.rows_repacked);
        assert_eq!(e.vk_format, "B8G8R8A8_UNORM");
        let bytes = std::fs::read(out.join(&e.file)).unwrap();
        assert_eq!(level0(&bytes).unwrap(), px.as_slice());
        let h = parse_header(&bytes).unwrap();
        assert_eq!(
            (h.depth, h.layer_count, h.face_count, h.level_count),
            (0, 0, 1, 1)
        );
        let kv = kv_pairs(&bytes);
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.streamRef" && v == "25")
        );
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.rowsRepacked" && v == "false")
        );
        assert!(
            kv.iter().all(|(k, _)| k != "gputrace.mipLevelCount"),
            "no descriptor keys without a descriptor"
        );
    }

    #[test]
    fn padded_rows_are_written_tight_and_flagged() {
        let mut bytes = vec![0xEE; 24 * 2];
        for y in 0..2 {
            for x in 0..2 {
                bytes[y * 24 + x * 4..y * 24 + x * 4 + 4]
                    .copy_from_slice(&[x as u8, y as u8, 7, 7]);
            }
        }
        let t = FakeTex::new(1, 2, 2, 24, 80, bytes);
        let (man, out) = run("padded", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert!(man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(
            level0(&file).unwrap(),
            &[0, 0, 7, 7, 1, 0, 7, 7, 0, 1, 7, 7, 1, 1, 7, 7]
        );
        assert!(
            kv_pairs(&file)
                .iter()
                .any(|(k, v)| k == "gputrace.rowsRepacked" && v == "true")
        );
    }

    #[test]
    fn compressed_blocks_pass_through_and_padded_block_rows_are_tightened() {
        // 8x8 ASTC 4x4: 2x2 blocks of 16 bytes, tight row 32.
        let blocks: Vec<u8> = (0..64).collect();
        let t = FakeTex::new(2, 8, 8, 32, 204, blocks.clone());
        let (man, out) = run("astc", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert_eq!(man.textures[0].file, "ref2_8x8_ASTC_4x4_LDR.ktx2");
        assert!(!man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(level0(&file).unwrap(), blocks.as_slice());
        assert_eq!(parse_header(&file).unwrap().vk_format, 157);
        // Same blocks at a 48-byte stride (one padding block per row).
        let mut padded = Vec::new();
        for r in 0..2 {
            padded.extend_from_slice(&blocks[r * 32..r * 32 + 32]);
            padded.extend_from_slice(&[0xAA; 16]);
        }
        let t = FakeTex::new(3, 8, 8, 48, 204, padded);
        let (man, out) = run("astc_padded", &plain(t, Aspect::Color));
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert!(man.textures[0].rows_repacked);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        assert_eq!(level0(&file).unwrap(), blocks.as_slice());
    }

    #[test]
    fn stencil_aspects_get_the_suffix_and_s8_uint() {
        let t = FakeTex::solid(4, 2, 2, 261, &[42]);
        let f = Fetched {
            texture: t,
            aspect: Aspect::Stencil,
            probed: true,
            descriptor: None,
        };
        let (man, _) = run("stencil", &f);
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        assert_eq!(man.textures[0].file, "ref4_2x2_X32_Stencil8_stencil.ktx2");
        assert_eq!(man.textures[0].vk_format, "S8_UINT");
        let t = FakeTex::solid(5, 2, 2, 253, &[42]);
        let (man, _) = run("stencil_base", &plain(t, Aspect::Stencil));
        assert_eq!(
            man.textures[0].file, "ref5_2x2_Stencil8.ktx2",
            "a base stencil texture has no suffix"
        );
    }

    #[test]
    fn unsupported_and_truncated_are_named_failures() {
        let t = FakeTex::solid(7, 2, 2, 90, &[0, 0, 0, 0]); // RGB10A2Unorm
        let (man, _) = run("packed", &plain(t, Aspect::Color));
        assert!(man.textures.is_empty());
        assert_eq!(man.failures[0].stream_ref, 7);
        assert!(
            man.failures[0].reason.contains("RGB10A2Unorm (90)"),
            "{}",
            man.failures[0].reason
        );
        assert!(man.failures[0].reason.contains("packed"));
        let t = FakeTex::new(8, 4, 4, 16, 70, vec![0; 8]);
        let (man, _) = run("short", &plain(t, Aspect::Color));
        assert!(
            man.failures[0].reason.contains("truncated"),
            "{}",
            man.failures[0].reason
        );
    }

    #[test]
    fn volumes_are_refused_only_when_certain_and_deeper_than_one() {
        let t = FakeTex::solid(9, 4, 4, 80, &[1, 2, 3, 4]);
        let deep = Fetched {
            texture: t.clone(),
            aspect: Aspect::Color,
            probed: false,
            descriptor: Some(Attributed {
                descriptor: desc(7, 4, 1, 1),
                attribution: Attribution::Certain,
            }),
        };
        let (man, _) = run("vol_certain", &deep);
        assert!(man.textures.is_empty());
        assert!(
            man.failures[0].reason.contains("3D texture, depth 4"),
            "{}",
            man.failures[0].reason
        );

        let flat = Fetched {
            texture: t.clone(),
            aspect: Aspect::Color,
            probed: false,
            descriptor: Some(Attributed {
                descriptor: desc(7, 1, 1, 1),
                attribution: Attribution::Certain,
            }),
        };
        let (man, out) = run("vol_depth1", &flat);
        assert!(man.failures.is_empty(), "{:?}", man.failures);
        let d = man.textures[0].descriptor.as_ref().unwrap();
        assert_eq!(d.texture_type, "3D");
        assert_eq!(d.attribution, Attribution::Certain);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        let kv = kv_pairs(&file);
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.textureType" && v == "3D")
        );
        assert!(kv.iter().any(|(k, v)| k == "gputrace.depth" && v == "1"));
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.descriptorAttribution" && v == "certain")
        );

        let ambiguous = Fetched {
            texture: t,
            aspect: Aspect::Color,
            probed: false,
            descriptor: Some(Attributed {
                descriptor: desc(7, 4, 1, 1),
                attribution: Attribution::Ambiguous,
            }),
        };
        let (man, _) = run("vol_ambiguous", &ambiguous);
        assert!(
            man.failures.is_empty(),
            "an ambiguous attribution never withholds bytes"
        );
        assert_eq!(
            man.textures[0].descriptor.as_ref().unwrap().attribution,
            Attribution::Ambiguous
        );
    }

    #[test]
    fn descriptor_keys_carry_mip_and_array_counts() {
        let t = FakeTex::solid(10, 4, 4, 80, &[1, 2, 3, 4]);
        let f = Fetched {
            texture: t,
            aspect: Aspect::Color,
            probed: false,
            descriptor: Some(Attributed {
                descriptor: desc(3, 1, 3, 6),
                attribution: Attribution::Certain,
            }),
        };
        let (man, out) = run("desc", &f);
        let file = std::fs::read(out.join(&man.textures[0].file)).unwrap();
        let kv = kv_pairs(&file);
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.mipLevelCount" && v == "3")
        );
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.arrayLength" && v == "6")
        );
        assert!(
            kv.iter()
                .any(|(k, v)| k == "gputrace.textureType" && v == "2DArray")
        );
        assert_eq!(man.textures[0].descriptor.as_ref().unwrap().mip_levels, 3);
    }
}
