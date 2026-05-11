use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{self, Write};

// ─── Tree ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Tree {
    Leaf { sym: u8, freq: u64 },
    Node { freq: u64, left: Box<Tree>, right: Box<Tree> },
}

impl Tree {
    fn freq(&self) -> u64 {
        match self { Tree::Leaf { freq, .. } | Tree::Node { freq, .. } => *freq }
    }
}

impl PartialEq for Tree { fn eq(&self, o: &Self) -> bool { self.freq() == o.freq() } }
impl Eq for Tree {}
impl PartialOrd for Tree { fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }
impl Ord for Tree { fn cmp(&self, o: &Self) -> std::cmp::Ordering { self.freq().cmp(&o.freq()) } }

// ─── Codebook ────────────────────────────────────────────────────────────────

// (code, bits): code è right-justified in u64, bits = lunghezza
pub type EncodeTable = [(u64, u8); 256];

pub fn build_encode_table(freqs: &[u64; 256]) -> EncodeTable {
    let mut table = [(0u64, 0u8); 256];

    let mut heap: BinaryHeap<Reverse<Box<Tree>>> = freqs
        .iter()
        .enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(i, &f)| Reverse(Box::new(Tree::Leaf { sym: i as u8, freq: f })))
        .collect();

    if heap.is_empty() {
        return table;
    }

    if heap.len() == 1 {
        let sym = match *heap.pop().unwrap().0 { Tree::Leaf { sym, .. } => sym, _ => 0 };
        table[sym as usize] = (0, 1);
        return table;
    }

    while heap.len() > 1 {
        let Reverse(a) = heap.pop().unwrap();
        let Reverse(b) = heap.pop().unwrap();
        heap.push(Reverse(Box::new(Tree::Node {
            freq: a.freq() + b.freq(),
            left: a,
            right: b,
        })));
    }

    fn assign(node: &Tree, code: u64, depth: u8, table: &mut EncodeTable) {
        match node {
            Tree::Leaf { sym, .. } => table[*sym as usize] = (code, depth),
            Tree::Node { left, right, .. } => {
                assign(left,  code << 1,       depth + 1, table);
                assign(right, (code << 1) | 1, depth + 1, table);
            }
        }
    }
    assign(&heap.pop().unwrap().0, 0, 0, &mut table);
    table
}

pub fn count_freqs(data: &[u8]) -> [u64; 256] {
    let mut f = [0u64; 256];
    for &b in data { f[b as usize] += 1; }
    f
}

// ─── Serialization ───────────────────────────────────────────────────────────

// Format: u16 n_symbols, then per symbol: sym(u8) + bits(u8) + code(u64 LE)
pub fn serialize_table(table: &EncodeTable) -> Vec<u8> {
    let entries: Vec<(u8, u64, u8)> = table.iter()
        .enumerate()
        .filter(|(_, (_, bits))| *bits > 0)
        .map(|(i, &(code, bits))| (i as u8, code, bits))
        .collect();

    let mut out = Vec::with_capacity(2 + entries.len() * 10);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (sym, code, bits) in &entries {
        out.push(*sym);
        out.push(*bits);
        out.extend_from_slice(&code.to_le_bytes());
    }
    out
}

pub fn deserialize_table(data: &[u8]) -> io::Result<(EncodeTable, usize)> {
    if data.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "codebook truncated"));
    }
    let n = u16::from_le_bytes([data[0], data[1]]) as usize;
    let needed = 2 + n * 10;
    if data.len() < needed {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "codebook truncated"));
    }
    let mut table = [(0u64, 0u8); 256];
    for i in 0..n {
        let off = 2 + i * 10;
        let sym  = data[off] as usize;
        let bits = data[off + 1];
        let code = u64::from_le_bytes(data[off+2..off+10].try_into().unwrap());
        table[sym] = (code, bits);
    }
    Ok((table, needed))
}

// ─── Decode tree (array trie per O(1) per bit) ───────────────────────────────

pub struct DecodeTree {
    // nodes[i] = [left_child, right_child] oppure [sym as u16, LEAF_MARK]
    nodes: Vec<[u16; 2]>,
}

const LEAF_MARK: u16 = 0xFFFF;
const NIL: u16 = 0xFFFE;

impl DecodeTree {
    pub fn build(table: &EncodeTable) -> Self {
        let mut nodes: Vec<[u16; 2]> = vec![[NIL, NIL]]; // root = index 0

        for (sym, &(code, bits)) in table.iter().enumerate() {
            if bits == 0 { continue; }
            let mut node = 0usize;
            for d in (0..bits).rev() {
                let bit = ((code >> d) & 1) as usize;
                if nodes[node][bit] == NIL {
                    nodes.push([NIL, NIL]);
                    nodes[node][bit] = (nodes.len() - 1) as u16;
                }
                node = nodes[node][bit] as usize;
            }
            nodes[node] = [sym as u16, LEAF_MARK];
        }
        Self { nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes[0] == [NIL, NIL]
    }
}

// ─── Encode ──────────────────────────────────────────────────────────────────

pub fn encode(data: &[u8], table: &EncodeTable, out: &mut impl Write) -> io::Result<()> {
    let mut buf: u64 = 0;
    let mut buf_len: u8 = 0;

    for &b in data {
        let (code, bits) = table[b as usize];
        buf = (buf << bits) | code;
        buf_len += bits;
        while buf_len >= 8 {
            buf_len -= 8;
            out.write_all(&[((buf >> buf_len) & 0xFF) as u8])?;
        }
    }
    if buf_len > 0 {
        out.write_all(&[((buf << (8 - buf_len)) & 0xFF) as u8])?;
    }
    Ok(())
}

// ─── Decode ──────────────────────────────────────────────────────────────────

pub fn decode(
    encoded: &[u8],
    tree: &DecodeTree,
    expected_len: usize,
    out: &mut impl Write,
) -> io::Result<()> {
    if tree.is_empty() {
        return Ok(());
    }

    // Single-symbol edge case: root is a leaf
    if tree.nodes[0][1] == LEAF_MARK {
        let sym = tree.nodes[0][0] as u8;
        for _ in 0..expected_len {
            out.write_all(&[sym])?;
        }
        return Ok(());
    }

    let mut node = 0usize;
    let mut written = 0usize;

    'outer: for &byte in encoded {
        for shift in (0..8).rev() {
            let bit = ((byte >> shift) & 1) as usize;
            node = tree.nodes[node][bit] as usize;
            if tree.nodes[node][1] == LEAF_MARK {
                out.write_all(&[tree.nodes[node][0] as u8])?;
                written += 1;
                if written == expected_len { break 'outer; }
                node = 0;
            }
        }
    }
    Ok(())
}

// ─── Combined: two-pass encode with serialized header ────────────────────────

pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
    let freqs = count_freqs(data);
    let table = build_encode_table(&freqs);
    let cb = serialize_table(&table);

    let mut encoded = Vec::new();
    encode(data, &table, &mut encoded)?;

    let mut out = Vec::with_capacity(2 + cb.len() + encoded.len());
    out.extend_from_slice(&(cb.len() as u16).to_le_bytes());
    out.extend_from_slice(&cb);
    out.extend_from_slice(&encoded);
    Ok(out)
}

pub fn decompress(data: &[u8], original_len: usize) -> io::Result<Vec<u8>> {
    if data.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "huffman header truncated"));
    }
    let cb_len = u16::from_le_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + cb_len {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "huffman codebook truncated"));
    }
    let (table, _) = deserialize_table(&data[2..2+cb_len])?;
    let tree = DecodeTree::build(&table);
    let mut out = Vec::with_capacity(original_len);
    decode(&data[2+cb_len..], &tree, original_len, &mut out)?;
    Ok(out)
}
