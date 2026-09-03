//! A minimal KTX2 writer (spec 7): one level, one layer, one face, no
//! supercompression. Header, index, level index, DFD, key/value data,
//! aligned level payload. Plus a reader used by tests to check what was
//! written without trusting the writer.

const IDENTIFIER: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];
const HEADER_LEN: u64 = 48;
const INDEX_LEN: u64 = 32;
const LEVEL_INDEX_LEN: u64 = 24; // one level
/// Where the DFD starts: header, index, one level-index entry.
pub const DFD_OFFSET: u64 = HEADER_LEN + INDEX_LEN + LEVEL_INDEX_LEN;

#[derive(Debug, Clone, Copy)]
pub struct Ktx2Params<'a> {
    pub vk_format: u32,
    pub type_size: u32,
    pub width: u32,
    pub height: u32,
    /// Bytes per texel block: bytes per pixel for uncompressed formats,
    /// `block_bytes` for compressed. Sets the level data alignment.
    pub texel_block_bytes: u32,
    pub dfd: &'a [u8],
    pub kv: &'a [(String, String)],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KtxError {
    #[error("dimension must be non-zero (got {width}x{height})")]
    ZeroDimension { width: u32, height: u32 },
    #[error("payload is empty")]
    EmptyPayload,
    #[error("texel block size must be non-zero")]
    ZeroBlock,
}

fn build_kvd(kv: &[(String, String)]) -> Vec<u8> {
    let mut entries: Vec<&(String, String)> = kv.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut out = Vec::new();
    for (k, v) in entries {
        let len = k.len() + 1 + v.len() + 1;
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        out.extend_from_slice(v.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

fn lcm4(n: u64) -> u64 {
    let (mut a, mut b) = (n, 4u64);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    n / a * 4
}

pub fn write_ktx2(p: &Ktx2Params, payload: &[u8]) -> Result<Vec<u8>, KtxError> {
    if p.width == 0 || p.height == 0 {
        return Err(KtxError::ZeroDimension {
            width: p.width,
            height: p.height,
        });
    }
    if payload.is_empty() {
        return Err(KtxError::EmptyPayload);
    }
    if p.texel_block_bytes == 0 {
        return Err(KtxError::ZeroBlock);
    }
    let kvd = build_kvd(p.kv);
    let kvd_off = DFD_OFFSET + p.dfd.len() as u64;
    let align = lcm4(u64::from(p.texel_block_bytes));
    let mut level_off = kvd_off + kvd.len() as u64;
    if !level_off.is_multiple_of(align) {
        level_off += align - (level_off % align);
    }

    let mut out = Vec::with_capacity(level_off as usize + payload.len());
    out.extend_from_slice(&IDENTIFIER);
    for v in [p.vk_format, p.type_size, p.width, p.height, 0, 0, 1, 1, 0] {
        // pixelDepth 0, layerCount 0, faceCount 1, levelCount 1, scheme 0
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(DFD_OFFSET as u32).to_le_bytes());
    out.extend_from_slice(&(p.dfd.len() as u32).to_le_bytes());
    out.extend_from_slice(&(if kvd.is_empty() { 0 } else { kvd_off as u32 }).to_le_bytes());
    out.extend_from_slice(&(kvd.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteOffset
    out.extend_from_slice(&0u64.to_le_bytes()); // sgdByteLength
    out.extend_from_slice(&level_off.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(p.dfd);
    out.extend_from_slice(&kvd);
    out.resize(level_off as usize, 0);
    out.extend_from_slice(payload);
    Ok(out)
}

/// The fields of a KTX2 header and index this tool cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub vk_format: u32,
    pub type_size: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub layer_count: u32,
    pub face_count: u32,
    pub level_count: u32,
    pub dfd_offset: u32,
    pub dfd_len: u32,
    pub kvd_offset: u32,
    pub kvd_len: u32,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Some(u32::from_le_bytes(a))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_le_bytes(a))
}

pub fn parse_header(b: &[u8]) -> Option<Header> {
    if b.get(..12)? != IDENTIFIER {
        return None;
    }
    Some(Header {
        vk_format: u32_at(b, 12)?,
        type_size: u32_at(b, 16)?,
        width: u32_at(b, 20)?,
        height: u32_at(b, 24)?,
        depth: u32_at(b, 28)?,
        layer_count: u32_at(b, 32)?,
        face_count: u32_at(b, 36)?,
        level_count: u32_at(b, 40)?,
        dfd_offset: u32_at(b, 48)?,
        dfd_len: u32_at(b, 52)?,
        kvd_offset: u32_at(b, 56)?,
        kvd_len: u32_at(b, 60)?,
    })
}

/// The level-0 payload bytes.
pub fn level0(b: &[u8]) -> Option<&[u8]> {
    let off = u64_at(b, 80)? as usize;
    let len = u64_at(b, 88)? as usize;
    b.get(off..off.checked_add(len)?)
}

/// The key/value pairs, in file order.
pub fn kv_pairs(b: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(h) = parse_header(b) else { return out };
    let (start, end) = (h.kvd_offset as usize, (h.kvd_offset + h.kvd_len) as usize);
    let mut at = start;
    while at + 4 <= end {
        let Some(len) = u32_at(b, at) else { break };
        let Some(pair) = b.get(at + 4..at + 4 + len as usize) else {
            break;
        };
        let mut parts = pair.split(|&c| c == 0);
        let k = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
        let v = String::from_utf8_lossy(parts.next().unwrap_or(&[])).into_owned();
        out.push((k, v));
        at += 4 + len as usize;
        while at % 4 != 0 {
            at += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dfd;
    use crate::vkformat::lookup;
    use gputools_replay_hl::MTLPixelFormat;

    fn bgra_params<'a>(dfd: &'a [u8], kv: &'a [(String, String)]) -> Ktx2Params<'a> {
        let vk = lookup(MTLPixelFormat::BGRA8Unorm).unwrap();
        Ktx2Params {
            vk_format: vk.value,
            type_size: vk.type_size,
            width: 4,
            height: 4,
            texel_block_bytes: 4,
            dfd,
            kv,
        }
    }

    #[test]
    fn header_fields_round_trip() {
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let out = write_ktx2(&bgra_params(&d, &[]), &[7u8; 64]).unwrap();
        let h = parse_header(&out).unwrap();
        assert_eq!(h.vk_format, 44);
        assert_eq!(h.type_size, 1);
        assert_eq!((h.width, h.height, h.depth), (4, 4, 0));
        assert_eq!((h.layer_count, h.face_count, h.level_count), (0, 1, 1));
        assert_eq!(h.dfd_offset as u64, DFD_OFFSET);
        assert_eq!(h.dfd_len as usize, d.len());
        assert_eq!((h.kvd_offset, h.kvd_len), (0, 0));
        assert_eq!(level0(&out).unwrap(), &[7u8; 64]);
    }

    #[test]
    fn kv_pairs_are_sorted_padded_and_read_back() {
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let kv = vec![
            ("gputrace.streamRef".to_string(), "25".to_string()),
            ("KTXwriter".to_string(), "ktx2-fetch 0.1.0".to_string()),
        ];
        let out = write_ktx2(&bgra_params(&d, &kv), &[0u8; 64]).unwrap();
        let read = kv_pairs(&out);
        assert_eq!(read[0].0, "KTXwriter"); // 'K' (0x4b) sorts before 'g'
        assert_eq!(
            read[1],
            ("gputrace.streamRef".to_string(), "25".to_string())
        );
        let h = parse_header(&out).unwrap();
        assert_eq!(h.kvd_offset % 4, 0);
        assert_eq!(h.kvd_len % 4, 0);
    }

    #[test]
    fn level_data_is_aligned_to_lcm_of_4_and_the_block_size() {
        for (block, want) in [(1u32, 4u64), (2, 4), (4, 4), (8, 8), (16, 16)] {
            let d = vec![0u8; 44];
            let kv = vec![("a".to_string(), "b".to_string())]; // 4+4 = 8 bytes: kvd length 8
            let p = Ktx2Params {
                vk_format: 1,
                type_size: 1,
                width: 1,
                height: 1,
                texel_block_bytes: block,
                dfd: &d,
                kv: &kv,
            };
            let out = write_ktx2(&p, &[1u8; 16]).unwrap();
            let off = u64_at(&out, 80).unwrap();
            assert_eq!(off % want, 0, "block {block}");
            assert_eq!(level0(&out).unwrap(), &[1u8; 16]);
        }
    }

    #[test]
    fn rejects_zero_dimensions_and_empty_payloads() {
        let d = vec![0u8; 44];
        let p = Ktx2Params {
            vk_format: 1,
            type_size: 1,
            width: 0,
            height: 4,
            texel_block_bytes: 1,
            dfd: &d,
            kv: &[],
        };
        assert_eq!(
            write_ktx2(&p, &[1]),
            Err(KtxError::ZeroDimension {
                width: 0,
                height: 4
            })
        );
        let p = Ktx2Params { width: 1, ..p };
        assert_eq!(write_ktx2(&p, &[]), Err(KtxError::EmptyPayload));
    }

    /// External oracle, only when Khronos' `ktx` is on PATH.
    #[test]
    fn a_written_file_passes_ktx_validate_when_available() {
        if std::process::Command::new("ktx")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("SKIP: ktx not on PATH");
            return;
        }
        let d = dfd::build(80, &lookup(MTLPixelFormat::BGRA8Unorm).unwrap()).unwrap();
        let kv = vec![
            ("KTXwriter".to_string(), "ktx2-fetch test".to_string()),
            ("gputrace.streamRef".to_string(), "1".to_string()),
        ];
        let out = write_ktx2(&bgra_params(&d, &kv), &[9u8; 64]).unwrap();
        let path =
            std::env::temp_dir().join(format!("ktx2_fetch_unit_{}.ktx2", std::process::id()));
        std::fs::write(&path, &out).unwrap();
        let st = std::process::Command::new("ktx")
            .arg("validate")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            st.status.success(),
            "{}{}",
            String::from_utf8_lossy(&st.stdout),
            String::from_utf8_lossy(&st.stderr)
        );
    }
}
