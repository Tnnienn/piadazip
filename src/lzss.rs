use std::io;

pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;

const NIL: u32 = u32::MAX;
const HASH_SIZE: usize = 1 << 16; // 65536

// ─── Config per livello 1-9 ──────────────────────────────────────────────────

pub struct LzssConfig {
    pub window_bits: u8,  // log2(window_size)
    pub max_chain: u32,
    pub lazy: bool,       // lazy matching (livelli alti)
}

impl LzssConfig {
    pub fn for_level(level: u8) -> Self {
        let level = level.clamp(1, 9);
        match level {
            1 => Self { window_bits: 12, max_chain: 4,   lazy: false },
            2 => Self { window_bits: 13, max_chain: 8,   lazy: false },
            3 => Self { window_bits: 13, max_chain: 16,  lazy: false },
            4 => Self { window_bits: 14, max_chain: 32,  lazy: false },
            5 => Self { window_bits: 14, max_chain: 64,  lazy: false },
            6 => Self { window_bits: 15, max_chain: 128, lazy: false },
            7 => Self { window_bits: 15, max_chain: 128, lazy: true  },
            8 => Self { window_bits: 15, max_chain: 256, lazy: true  },
            _ => Self { window_bits: 15, max_chain: 512, lazy: true  },
        }
    }

    pub fn window_size(&self) -> usize {
        1 << self.window_bits
    }
}

// ─── Hash chain ──────────────────────────────────────────────────────────────

struct HashChain {
    head: Vec<u32>,  // head[hash] = ultima posizione con quel hash
    prev: Vec<u32>,  // prev[pos % window] = posizione precedente con stesso hash
    window: usize,
}

impl HashChain {
    fn new(window: usize) -> Self {
        Self {
            head: vec![NIL; HASH_SIZE],
            prev: vec![NIL; window],
            window,
        }
    }

    fn hash(data: &[u8], pos: usize) -> usize {
        let b0 = data[pos]     as usize;
        let b1 = data[pos + 1] as usize;
        let b2 = data[pos + 2] as usize;
        (b0 ^ (b1 << 5) ^ (b2 << 13)) & (HASH_SIZE - 1)
    }

    fn insert(&mut self, data: &[u8], pos: usize) {
        let h = Self::hash(data, pos);
        self.prev[pos % self.window] = self.head[h];
        self.head[h] = pos as u32;
    }

    fn find_match(
        &self,
        data: &[u8],
        pos: usize,
        max_chain: u32,
        prev_match_len: usize, // minimo da battere (lazy matching)
    ) -> Option<(usize, usize)> {  // (offset, length)
        if pos + MIN_MATCH > data.len() {
            return None;
        }
        let h = Self::hash(data, pos);
        let limit = if pos >= self.window { pos - self.window } else { 0 };

        let mut best_len = prev_match_len;
        let mut best_pos = 0usize;
        let mut candidate = self.head[h];
        let mut chain_count = 0u32;

        while candidate != NIL && chain_count < max_chain {
            let cpos = candidate as usize;
            if cpos >= pos || cpos <= limit { break; }

            let max_len = (data.len() - pos).min(MAX_MATCH);
            // ottimizzazione: salta candidato se il byte in posizione best_len non corrisponde
            if max_len > best_len
                && cpos + best_len < data.len()
                && data[cpos + best_len] != data[pos + best_len]
            {
                candidate = self.prev[cpos % self.window];
                chain_count += 1;
                continue;
            }

            let mut len = 0;
            while len < max_len && data[cpos + len] == data[pos + len] {
                len += 1;
            }

            if len > best_len {
                best_len = len;
                best_pos = cpos;
                if len == MAX_MATCH { break; }
            }

            candidate = self.prev[cpos % self.window];
            chain_count += 1;
        }

        if best_len >= MIN_MATCH {
            Some((pos - best_pos, best_len))
        } else {
            None
        }
    }
}

// ─── Token stream ─────────────────────────────────────────────────────────────
//
// Gruppo da 8 token:
//   1 byte flags (bit7 = token[0], ..., bit0 = token[7])
//   Per ogni token:
//     flag=0 → literal: 1 byte
//     flag=1 → match: [offset_lo][offset_hi (bits 14..7)][length - MIN_MATCH]
//              offset è 1-based (1 = byte immediatamente precedente)
//              max offset = 32767 (15 bit)

// ─── Encode ──────────────────────────────────────────────────────────────────

pub fn encode(data: &[u8], config: &LzssConfig) -> Vec<u8> {
    if data.len() < MIN_MATCH {
        // file troppo corto per trovare match: tutto literal
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let end = (pos + 8).min(data.len());
            let count = end - pos;
            let flags = 0u8;
            out.push(flags);
            out.extend_from_slice(&data[pos..end]);
            pos += count;
        }
        return out;
    }

    let window = config.window_size();
    let max_chain = config.max_chain;
    let lazy = config.lazy;

    let mut chain = HashChain::new(window);
    let mut out = Vec::with_capacity(data.len() / 2);

    // buffer per il gruppo corrente (max 8 token * 3 byte = 24 byte)
    let mut group_buf = [0u8; 24];
    let mut group_len = 0usize;
    let mut flags = 0u8;
    let mut token_count = 0u8;

    let flush = |flags: u8, group_buf: &[u8], group_len: usize, out: &mut Vec<u8>| {
        out.push(flags);
        out.extend_from_slice(&group_buf[..group_len]);
    };

    let mut pos = 0;
    while pos < data.len() {
        // inserisci nella hash chain solo se ci sono abbastanza byte
        let can_hash = pos + MIN_MATCH <= data.len();

        let m = if can_hash {
            let result = chain.find_match(data, pos, max_chain, MIN_MATCH - 1);
            chain.insert(data, pos);
            result
        } else {
            None
        };

        let (use_offset, use_len) = if let Some((offset, len)) = m {
            if lazy && pos + 1 + MIN_MATCH <= data.len() {
                // guarda un passo avanti
                if pos + 1 + MIN_MATCH <= data.len() {
                    chain.insert(data, pos + 1);
                }
                let m2 = chain.find_match(data, pos + 1, max_chain, len);
                if let Some((off2, len2)) = m2 {
                    if len2 > len {
                        // emetti literal e usa il match avanzato
                        let b = data[pos];
                        flags |= 0 << (7 - token_count);
                        group_buf[group_len] = b;
                        group_len += 1;
                        token_count += 1;
                        if token_count == 8 {
                            flush(flags, &group_buf, group_len, &mut out);
                            flags = 0; group_len = 0; token_count = 0;
                        }
                        pos += 1;
                        (off2, len2)
                    } else {
                        (offset, len)
                    }
                } else {
                    (offset, len)
                }
            } else {
                (offset, len)
            }
        } else {
            // nessun match: literal
            let b = data[pos];
            flags |= 0 << (7 - token_count);
            group_buf[group_len] = b;
            group_len += 1;
            token_count += 1;
            if token_count == 8 {
                flush(flags, &group_buf, group_len, &mut out);
                flags = 0; group_len = 0; token_count = 0;
            }
            pos += 1;
            continue;
        };

        // emetti match
        flags |= 1 << (7 - token_count);
        let off_enc = (use_offset as u16).to_le_bytes();
        group_buf[group_len]     = off_enc[0];
        group_buf[group_len + 1] = off_enc[1];
        group_buf[group_len + 2] = (use_len - MIN_MATCH) as u8;
        group_len += 3;
        token_count += 1;

        if token_count == 8 {
            flush(flags, &group_buf, group_len, &mut out);
            flags = 0; group_len = 0; token_count = 0;
        }

        // inserisci nella hash chain i byte saltati dal match
        let end = (pos + use_len).min(data.len());
        for p in (pos + 1)..end {
            if p + MIN_MATCH <= data.len() {
                chain.insert(data, p);
            }
        }
        pos += use_len;
    }

    // flush gruppo finale
    if token_count > 0 {
        flush(flags, &group_buf, group_len, &mut out);
    }

    out
}

// ─── Decode ──────────────────────────────────────────────────────────────────

pub fn decode(data: &[u8], original_len: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(original_len);
    let mut i = 0;

    while i < data.len() && out.len() < original_len {
        let flags = data[i];
        i += 1;

        for bit in (0..8).rev() {
            if out.len() >= original_len { break; }
            if i >= data.len() { break; }

            if (flags >> bit) & 1 == 0 {
                // literal
                out.push(data[i]);
                i += 1;
            } else {
                // match
                if i + 2 >= data.len() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "lzss: match token troncato"));
                }
                let offset = u16::from_le_bytes([data[i], data[i+1]]) as usize;
                let length = data[i+2] as usize + MIN_MATCH;
                i += 3;

                if offset == 0 || offset > out.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("lzss: offset invalido {} (decoded so far: {})", offset, out.len()),
                    ));
                }

                let start = out.len() - offset;
                // copia byte per byte per supportare match sovrapposti (run encoding)
                for k in 0..length {
                    let b = out[start + k];
                    out.push(b);
                }
            }
        }
    }

    if out.len() != original_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("lzss: lunghezza attesa {}, ottenuta {}", original_len, out.len()),
        ));
    }
    Ok(out)
}
