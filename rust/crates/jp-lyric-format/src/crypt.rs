//! QQ 音乐 QRC 和酷狗 KRC 的加密外壳：解密、解压，得到明文歌词。
//!
//! 协议参考（2026-10-22 看过的 main）：
//! https://github.com/chenmozhijin/LDDC/tree/84631e8cd011fcc3f71ca0ae017e2c9758958ffc/LDDC/core
//! QQ 的 DES 有两处非标准 S 盒，而且按小端 32 位字排。下面的查表实现是独立写的，
//! 表本身是协议常量。LDDC 是 GPL-3.0-only，本模块是 GPL-3.0-or-later；
//! 以后要照抄上游实现，得保留它的声明和许可条件。

use std::io::Read;

use anyhow::{Result, anyhow, bail, ensure};
use flate2::read::ZlibDecoder;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const QRC_KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";
const KRC_KEY: &[u8; 16] = b"@Gaw^2tGQ61-\xce\xd2ni";
/// 本地 QRC 文件（QQ 音乐客户端缓存）的文件头
pub const LOCAL_QRC_HEADER: &[u8] = b"\x98%\xb0\xac\xe3\x02\x83h\xe8\xfcl";

fn inflate(bytes: &[u8]) -> Result<String> {
    let mut decoder = ZlibDecoder::new(bytes).take((MAX_BYTES + 1) as u64);
    let mut plain = Vec::new();
    decoder
        .read_to_end(&mut plain)
        .map_err(|_| anyhow!("歌词 zlib 解压失败或密文格式不受支持"))?;
    ensure!(plain.len() <= MAX_BYTES, "解压后的歌词超过 8 MiB 限制");
    String::from_utf8(plain).map_err(|_| anyhow!("解码后的歌词不是 UTF-8"))
}

/// Decode a binary KRC file (`krc1` + XOR-obfuscated zlib stream).
/// Network JSON content must be base64-decoded by the caller first.
pub fn decode_krc(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() <= MAX_BYTES, "KRC 文件过大");
    ensure!(
        bytes.starts_with(b"krc1") && bytes.len() > 4,
        "不支持的 KRC：需要 krc1 二进制文件头；普通 LRC 请走文本解析"
    );
    let packed: Vec<u8> = bytes[4..]
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ KRC_KEY[i % 16])
        .collect();
    inflate(&packed)
}

/// Decode network QRC encrypted bytes (already hex-decoded), or the known local
/// QMC1-wrapped QRC binary format. This function never guesses plain LRC/XML.
pub fn decode_qrc(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= MAX_BYTES && !bytes.is_empty(),
        "QRC 为空或超过大小限制"
    );
    let local;
    let encrypted = if bytes.starts_with(LOCAL_QRC_HEADER) {
        local = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let j = if i > 0x7fff { i % 0x7fff } else { i };
                b ^ QMC1[j & 127]
            })
            .collect::<Vec<_>>();
        &local[LOCAL_QRC_HEADER.len()..]
    } else {
        if let Ok(text) = std::str::from_utf8(bytes) {
            let text = text.trim_start_matches('\u{feff}').trim_start();
            ensure!(
                !text.starts_with('<') && !text.starts_with('['),
                "QRC/LRC 明文应走文本解析，不能作为 QRC 密文解码"
            );
        }
        bytes
    };
    ensure!(
        !encrypted.is_empty() && encrypted.len().is_multiple_of(8),
        "不支持的 QRC 密文长度或未知本地 QRC 头（需要 8 字节对齐）"
    );
    let keys = [
        des_keys(&QRC_KEY[16..]),
        des_keys(&QRC_KEY[8..16]),
        des_keys(&QRC_KEY[..8]),
    ];
    let mut packed = Vec::with_capacity(encrypted.len());
    for block in encrypted.as_chunks::<8>().0 {
        let mut b = *block;
        b = des_block(b, &keys[0], true);
        b = des_block(b, &keys[1], false);
        b = des_block(b, &keys[2], true);
        packed.extend_from_slice(&b);
    }
    inflate(&packed)
}

pub fn unhex(text: &str) -> Result<Vec<u8>> {
    ensure!(
        text.len() <= MAX_BYTES * 2 && text.len().is_multiple_of(2),
        "QRC hex 长度无效"
    );
    fn nibble(b: u8) -> Result<u8> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => bail!("QRC 密文不是合法 hex"),
        }
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| Ok(nibble(b[0])? * 16 + nibble(b[1])?))
        .collect()
}

// Generic DES bit permutations, expressed as one-based MSB positions. The QQ
// wire format reverses each 32-bit word on BOTH block and key boundaries.
fn permute(input: u64, width: u8, table: &[u8]) -> u64 {
    table
        .iter()
        .fold(0, |v, p| (v << 1) | ((input >> (width - *p)) & 1))
}
fn wire_word(mut b: [u8; 8]) -> [u8; 8] {
    b[..4].reverse();
    b[4..].reverse();
    b
}
fn des_keys(key: &[u8]) -> [u64; 16] {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(key);
    let k = permute(u64::from_be_bytes(wire_word(bytes)), 64, &PC1);
    let mut c = (k >> 28) as u32;
    let mut d = (k & 0x0fff_ffff) as u32;
    let mut out = [0; 16];
    for (i, shift) in [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1]
        .iter()
        .enumerate()
    {
        c = ((c << shift) | (c >> (28 - shift))) & 0x0fff_ffff;
        d = ((d << shift) | (d >> (28 - shift))) & 0x0fff_ffff;
        // QQ's key-compression D indices are displaced by one bit. Keep PC2 as
        // the standard table and select the D half separately; its final bit is
        // the zero just below the 28-bit register. Standard TripleDES is wrong.
        out[i] = PC2.iter().fold(0, |key, &position| {
            let bit = if position <= 28 {
                (c >> (28 - position)) & 1
            } else {
                let d_position = position - 28;
                if d_position < 28 {
                    (d >> (27 - d_position)) & 1
                } else {
                    0
                }
            };
            (key << 1) | bit as u64
        });
    }
    out
}
fn des_block(block: [u8; 8], keys: &[u64; 16], decrypt: bool) -> [u8; 8] {
    let ip = permute(u64::from_be_bytes(wire_word(block)), 64, &IP);
    let mut l = (ip >> 32) as u32;
    let mut r = ip as u32;
    for round in 0..16 {
        let expanded =
            permute(r as u64, 32, &EXPAND) ^ keys[if decrypt { 15 - round } else { round }];
        let mut substituted = 0u32;
        for (i, table) in SBOX.iter().enumerate() {
            let six = ((expanded >> (42 - i * 6)) & 63) as usize;
            let row = ((six & 32) >> 4) | (six & 1);
            let col = (six >> 1) & 15;
            substituted = (substituted << 4) | table[row * 16 + col] as u32;
        }
        let next = l ^ permute(substituted as u64, 32, &P) as u32;
        l = r;
        r = next;
    }
    wire_word(permute(((r as u64) << 32) | l as u64, 64, &FP).to_be_bytes())
}

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const EXPAND: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    // QQ: S2 row 1 col 7 is 15 (standard DES says 14).
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    // QQ: S4 row 3 col 5 is 10 (standard DES says 1).
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10,
        10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];
const QMC1: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];


/// 把明文压成 `krc1` 二进制（[`decode_krc`] 的逆运算）。造夹具、测试用。
pub fn encode_krc(text: &str) -> Vec<u8> {
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(text.as_bytes()).expect("in-memory zlib");
    let mut out = b"krc1".to_vec();
    out.extend(z.finish().expect("in-memory zlib").iter().enumerate().map(|(i, b)| b ^ KRC_KEY[i % 16]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const QRC_HEX: &str = "ee7befb746f93831d42dcdf2e21f0d5d833c044042289012e0c11abf870c7dfb9c809065c5235ff1cf6f94cef240c69add963e0164dd48ddbfc31abddb1b167277d92192f367e70a22f0ba7ac5e81564dd377778fc4103ff";
    pub(crate) const QRC_TEXT: &str = "<QrcInfos><Lyric_1 LyricType=\"1\" LyricContent=\"[1000,600]hi(1000,200)!(1200,400)\"/></QrcInfos>";

    #[test]
    fn qrc_matches_pinned_lddc_reference_vector() {
        assert_eq!(decode_qrc(&unhex(QRC_HEX).unwrap()).unwrap(), QRC_TEXT);
        let k = [
            des_keys(&QRC_KEY[..8]),
            des_keys(&QRC_KEY[8..16]),
            des_keys(&QRC_KEY[16..]),
        ];
        let b = des_block(
            des_block(
                des_block([0, 1, 2, 3, 4, 5, 6, 7], &k[0], false),
                &k[1],
                true,
            ),
            &k[2],
            false,
        );
        assert_eq!(b.to_vec(), unhex("ce92cac4ea3c8406").unwrap());
    }
    #[test]
    fn local_qrc_qmc1_wrapper_is_supported() {
        let mut packed = vec![0; 11];
        packed.extend(unhex(QRC_HEX).unwrap());
        for (i, b) in packed.iter_mut().enumerate() {
            *b ^= QMC1[i & 127];
        }
        packed[..11].copy_from_slice(LOCAL_QRC_HEADER);
        assert_eq!(decode_qrc(&packed).unwrap(), QRC_TEXT);
    }
    #[test]
    fn krc_binary_decoding_and_plain_text_rejection() {
        let plain = "[1000,600]<0,200,0>hi<200,400,0>!";
        assert_eq!(decode_krc(&encode_krc(plain)).unwrap(), plain);
        assert!(decode_krc(b"[00:01.00]hello").is_err());
        assert!(decode_qrc(b"[00:01.00]hello").is_err());
        assert!(decode_qrc(QRC_TEXT.as_bytes()).is_err());
        assert!(decode_qrc(b"bad").is_err());
        assert!(unhex("0z").is_err());
    }
}
