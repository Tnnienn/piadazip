use std::io;
use crate::{huffman, lzss};

// ─── Formato blob ─────────────────────────────────────────────────────────────
//
// [0..8]   orig_len    u64 LE
// [8..12]  lzss_len    u32 LE   (0 se method=store)
// [12]     method      u8       0=store  1=lzss_only  2=lzss+huffman
// [13..15] cb_len      u16 LE   (0 se method != 2)
// [15 ..15+cb_len]  codebook
// [15+cb_len ..]    encoded data

const METHOD_STORE:   u8 = 0;
const METHOD_LZSS:    u8 = 1;
const METHOD_LZSS_HUF: u8 = 2;

pub struct Config {
    pub level: u8,           // 1-9
    pub store_threshold: f64, // es. 0.95 — se ratio > threshold usa store
}

impl Default for Config {
    fn default() -> Self { Self { level: 6, store_threshold: 0.95 } }
}

impl Config {
    pub fn new(level: u8) -> Self {
        Self { level: level.clamp(1, 9), ..Default::default() }
    }
}

fn write_header(orig_len: u64, lzss_len: u32, method: u8, cb_len: u16) -> [u8; 15] {
    let mut h = [0u8; 15];
    h[0..8].copy_from_slice(&orig_len.to_le_bytes());
    h[8..12].copy_from_slice(&lzss_len.to_le_bytes());
    h[12] = method;
    h[13..15].copy_from_slice(&cb_len.to_le_bytes());
    h
}

pub fn compress(data: &[u8], config: &Config) -> io::Result<Vec<u8>> {
    let orig_len = data.len() as u64;

    if data.is_empty() {
        let mut out = Vec::new();
        out.extend_from_slice(&write_header(0, 0, METHOD_STORE, 0));
        return Ok(out);
    }

    // LZSS
    let lzss_cfg = lzss::LzssConfig::for_level(config.level);
    let token_stream = lzss::encode(data, &lzss_cfg);
    let lzss_len = token_stream.len() as u32;

    // Huffman su token stream
    let huf = huffman::compress(&token_stream)?;

    // Determina se usare store
    let ratio = (15 + huf.len()) as f64 / (data.len() as f64);
    if ratio > config.store_threshold {
        // store raw
        let mut out = Vec::with_capacity(15 + data.len());
        out.extend_from_slice(&write_header(orig_len, 0, METHOD_STORE, 0));
        out.extend_from_slice(data);
        return Ok(out);
    }

    // cb_len è nei primi 2 byte di huf
    let cb_len = u16::from_le_bytes([huf[0], huf[1]]);

    let mut out = Vec::with_capacity(15 + huf.len());
    out.extend_from_slice(&write_header(orig_len, lzss_len, METHOD_LZSS_HUF, cb_len));
    out.extend_from_slice(&huf[2..]); // salta i 2 byte di cb_len (già in header)
    Ok(out)
}

pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.len() < 15 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "blob header troncato"));
    }
    let orig_len  = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let lzss_len  = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let method    = data[12];
    let cb_len    = u16::from_le_bytes([data[13], data[14]]) as usize;
    let payload   = &data[15..];

    match method {
        METHOD_STORE => {
            Ok(payload[..orig_len].to_vec())
        }
        METHOD_LZSS_HUF => {
            // ricostruisci il formato che huffman::decompress si aspetta
            // (2 byte cb_len + codebook + encoded)
            let mut huf_input = Vec::with_capacity(2 + payload.len());
            huf_input.extend_from_slice(&(cb_len as u16).to_le_bytes());
            huf_input.extend_from_slice(payload);

            let token_stream = huffman::decompress(&huf_input, lzss_len)?;
            lzss::decode(&token_stream, orig_len)
        }
        METHOD_LZSS => {
            lzss::decode(payload, orig_len)
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("metodo di compressione sconosciuto: {}", method))),
    }
}
