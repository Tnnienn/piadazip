use std::collections::{BinaryHeap, HashMap};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::cmp::Reverse;

// ─── Format constants ────────────────────────────────────────────────────────

const MAGIC: &[u8; 8] = b"PIADINA\0";
const VERSION: u8 = 2;

// ─── RLE ─────────────────────────────────────────────────────────────────────

const ESCAPE: u8 = 0xFF;

fn rle_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == b && run < 255 {
            run += 1;
        }
        if run >= 3 || b == ESCAPE {
            out.push(ESCAPE);
            out.push(run as u8);
            out.push(b);
        } else {
            for _ in 0..run {
                out.push(b);
            }
        }
        i += run;
    }
    out
}

fn rle_decode(data: &[u8], original_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(original_len);
    let mut i = 0;
    while i < data.len() {
        if data[i] == ESCAPE {
            if i + 2 >= data.len() { break; }
            let count = data[i + 1] as usize;
            let byte  = data[i + 2];
            for _ in 0..count { out.push(byte); }
            i += 3;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

// ─── Huffman ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum HuffTree {
    Leaf { sym: u8, freq: usize },
    Node { freq: usize, left: Box<HuffTree>, right: Box<HuffTree> },
}

impl HuffTree {
    fn freq(&self) -> usize {
        match self { HuffTree::Leaf { freq, .. } | HuffTree::Node { freq, .. } => *freq }
    }
}

impl PartialEq for HuffTree { fn eq(&self, o: &Self) -> bool { self.freq() == o.freq() } }
impl Eq for HuffTree {}
impl PartialOrd for HuffTree { fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }
impl Ord for HuffTree { fn cmp(&self, o: &Self) -> std::cmp::Ordering { self.freq().cmp(&o.freq()) } }

fn build_codebook(data: &[u8]) -> HashMap<u8, (u64, u8)> {
    if data.is_empty() { return HashMap::new(); }

    let mut freq = [0usize; 256];
    for &b in data { freq[b as usize] += 1; }

    let mut heap: BinaryHeap<Reverse<Box<HuffTree>>> = freq
        .iter()
        .enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(i, &f)| Reverse(Box::new(HuffTree::Leaf { sym: i as u8, freq: f })))
        .collect();

    // single-symbol edge case
    if heap.len() == 1 {
        let sym = match *heap.pop().unwrap().0 { HuffTree::Leaf { sym, .. } => sym, _ => 0 };
        let mut m = HashMap::new();
        m.insert(sym, (0u64, 1u8));
        return m;
    }

    while heap.len() > 1 {
        let Reverse(a) = heap.pop().unwrap();
        let Reverse(b) = heap.pop().unwrap();
        let merged = Box::new(HuffTree::Node {
            freq: a.freq() + b.freq(),
            left: a,
            right: b,
        });
        heap.push(Reverse(merged));
    }

    let mut codebook = HashMap::new();
    fn traverse(node: &HuffTree, code: u64, depth: u8, cb: &mut HashMap<u8, (u64, u8)>) {
        match node {
            HuffTree::Leaf { sym, .. } => { cb.insert(*sym, (code, depth)); }
            HuffTree::Node { left, right, .. } => {
                traverse(left,  (code << 1),     depth + 1, cb);
                traverse(right, (code << 1) | 1, depth + 1, cb);
            }
        }
    }
    traverse(&heap.pop().unwrap().0, 0, 0, &mut codebook);
    codebook
}

fn huffman_encode(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let codebook = build_codebook(data);

    // serialize codebook: u16 n_symbols, then (sym, bits_len, code_u64) each
    let mut cb_bytes: Vec<u8> = Vec::new();
    let n = codebook.len() as u16;
    cb_bytes.extend_from_slice(&n.to_le_bytes());
    let mut entries: Vec<(u8, (u64, u8))> = codebook.iter().map(|(&s, &c)| (s, c)).collect();
    entries.sort_by_key(|(s, _)| *s);
    for (sym, (code, bits)) in &entries {
        cb_bytes.push(*sym);
        cb_bytes.push(*bits);
        cb_bytes.extend_from_slice(&code.to_le_bytes());
    }

    // encode bits
    let mut out: Vec<u8> = Vec::new();
    let mut buf: u64 = 0;
    let mut buf_len: u8 = 0;
    for &b in data {
        let (code, bits) = codebook[&b];
        buf = (buf << bits) | code;
        buf_len += bits;
        while buf_len >= 8 {
            buf_len -= 8;
            out.push(((buf >> buf_len) & 0xFF) as u8);
        }
    }
    if buf_len > 0 {
        out.push(((buf << (8 - buf_len)) & 0xFF) as u8);
    }

    (cb_bytes, out)
}

fn huffman_decode(cb_bytes: &[u8], enc: &[u8], original_len: usize) -> Vec<u8> {
    let n_sym = u16::from_le_bytes([cb_bytes[0], cb_bytes[1]]) as usize;
    let mut codebook: HashMap<u8, (u64, u8)> = HashMap::new();
    for i in 0..n_sym {
        let off = 2 + i * 10;
        let sym  = cb_bytes[off];
        let bits = cb_bytes[off + 1];
        let code = u64::from_le_bytes(cb_bytes[off+2..off+10].try_into().unwrap());
        codebook.insert(sym, (code, bits));
    }

    // build decode table: (code, bits) -> sym
    let decode_map: HashMap<(u64, u8), u8> = codebook.iter()
        .map(|(&s, &(c, b))| ((c, b), s))
        .collect();

    let mut out = Vec::with_capacity(original_len);
    let mut buf: u64 = 0;
    let mut buf_len: u8 = 0;
    let max_bits = codebook.values().map(|(_, b)| *b).max().unwrap_or(0);

    'outer: for &byte in enc {
        buf = (buf << 8) | byte as u64;
        buf_len += 8;
        while buf_len >= 1 {
            let mut found = false;
            for bits in 1..=max_bits.min(buf_len) {
                let shift = buf_len - bits;
                let code = (buf >> shift) & ((1u64 << bits) - 1);
                if let Some(&sym) = decode_map.get(&(code, bits)) {
                    out.push(sym);
                    buf_len -= bits;
                    buf &= (1u64 << buf_len) - 1;
                    found = true;
                    if out.len() == original_len { break 'outer; }
                    break;
                }
            }
            if !found { break; }
        }
    }
    out
}

// ─── Compress / decompress a single blob ─────────────────────────────────────

fn compress_bytes(data: &[u8]) -> Vec<u8> {
    let rle = rle_encode(data);
    let (cb, enc) = huffman_encode(&rle);

    let mut out = Vec::new();
    let original_len = data.len() as u64;
    let rle_len      = rle.len() as u64;
    let cb_len       = cb.len() as u32;

    out.extend_from_slice(&original_len.to_le_bytes()); // 8
    out.extend_from_slice(&rle_len.to_le_bytes());       // 8
    out.extend_from_slice(&cb_len.to_le_bytes());        // 4
    out.extend_from_slice(&cb);
    out.extend_from_slice(&enc);
    out
}

fn decompress_bytes(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.len() < 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too short"));
    }
    let original_len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let rle_len      = u64::from_le_bytes(data[8..16].try_into().unwrap()) as usize;
    let cb_len       = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
    let cb_start = 20;
    let enc_start = cb_start + cb_len;
    let cb_bytes = &data[cb_start..enc_start];
    let enc      = &data[enc_start..];
    let rle = huffman_decode(cb_bytes, enc, rle_len);
    Ok(rle_decode(&rle, original_len))
}

// ─── Archive entry ────────────────────────────────────────────────────────────

struct Entry {
    path: String,
    is_dir: bool,
    mode: u32,
    mtime: u64,
    data_offset: u64,
    data_len: u64,
}

fn write_u8(w: &mut impl Write, v: u8)   -> io::Result<()> { w.write_all(&[v]) }
fn write_u32(w: &mut impl Write, v: u32) -> io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn write_u64(w: &mut impl Write, v: u64) -> io::Result<()> { w.write_all(&v.to_le_bytes()) }
fn read_u8(r: &mut impl Read)   -> io::Result<u8>  { let mut b=[0u8;1]; r.read_exact(&mut b)?; Ok(b[0]) }
fn read_u32(r: &mut impl Read)  -> io::Result<u32> { let mut b=[0u8;4]; r.read_exact(&mut b)?; Ok(u32::from_le_bytes(b)) }
fn read_u64(r: &mut impl Read)  -> io::Result<u64> { let mut b=[0u8;8]; r.read_exact(&mut b)?; Ok(u64::from_le_bytes(b)) }

fn write_entry_header(w: &mut impl Write, e: &Entry) -> io::Result<()> {
    let path_bytes = e.path.as_bytes();
    write_u32(w, path_bytes.len() as u32)?;
    w.write_all(path_bytes)?;
    write_u8(w, e.is_dir as u8)?;
    write_u32(w, e.mode)?;
    write_u64(w, e.mtime)?;
    write_u64(w, e.data_offset)?;
    write_u64(w, e.data_len)?;
    Ok(())
}

fn read_entry_header(r: &mut impl Read) -> io::Result<Entry> {
    let path_len = read_u32(r)? as usize;
    let mut path_bytes = vec![0u8; path_len];
    r.read_exact(&mut path_bytes)?;
    let path   = String::from_utf8(path_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid utf8 path"))?;
    let is_dir      = read_u8(r)? != 0;
    let mode        = read_u32(r)?;
    let mtime       = read_u64(r)?;
    let data_offset = read_u64(r)?;
    let data_len    = read_u64(r)?;
    Ok(Entry { path, is_dir, mode, mtime, data_offset, data_len })
}

// ─── Collect files from paths ─────────────────────────────────────────────────

fn collect_files(src: &Path) -> io::Result<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    collect_recursive(src, src, &mut files)?;
    Ok(files)
}

fn collect_recursive(base: &Path, current: &Path, out: &mut Vec<(PathBuf, String)>) -> io::Result<()> {
    let name = if base == current {
        base.file_name()
            .unwrap_or(base.as_os_str())
            .to_string_lossy()
            .to_string()
    } else {
        current.strip_prefix(base.parent().unwrap_or(Path::new("")))
            .unwrap_or(current)
            .to_string_lossy()
            .to_string()
    };

    if current.is_dir() {
        out.push((current.to_path_buf(), name.clone()));
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            collect_recursive(base, &entry.path(), out)?;
        }
    } else {
        out.push((current.to_path_buf(), name));
    }
    Ok(())
}

fn sanitize_path(p: &str) -> Option<PathBuf> {
    let path = PathBuf::from(p);
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return None,
            std::path::Component::RootDir   => return None,
            _ => {}
        }
    }
    Some(path)
}

// ─── Commands ─────────────────────────────────────────────────────────────────

fn cmd_create(archive: &str, sources: &[&str]) -> io::Result<()> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    let mut offset: u64 = 0;

    for src in sources {
        let src_path = Path::new(src);
        if !src_path.exists() {
            eprintln!("warning: {} non trovato, saltato", src);
            continue;
        }
        let files = collect_files(src_path)?;
        for (path, name) in files {
            let meta = fs::metadata(&path)?;
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            #[cfg(unix)]
            let mode = std::os::unix::fs::MetadataExt::mode(&meta);
            #[cfg(not(unix))]
            let mode: u32 = if meta.is_dir() { 0o755 } else { 0o644 };

            if meta.is_dir() {
                entries.push(Entry { path: name, is_dir: true, mode, mtime, data_offset: 0, data_len: 0 });
                payloads.push(vec![]);
            } else {
                let raw = fs::read(&path)?;
                let compressed = compress_bytes(&raw);
                let len = compressed.len() as u64;
                entries.push(Entry { path: name, is_dir: false, mode, mtime, data_offset: offset, data_len: len });
                offset += len;
                payloads.push(compressed);
            }
        }
    }

    // write archive
    let mut f = File::create(archive)?;
    f.write_all(MAGIC)?;
    write_u8(&mut f, VERSION)?;
    write_u32(&mut f, entries.len() as u32)?;
    for e in &entries {
        write_entry_header(&mut f, e)?;
    }
    for p in &payloads {
        f.write_all(p)?;
    }

    let total_compressed: u64 = payloads.iter().map(|p| p.len() as u64).sum();
    let total_original: u64 = entries.iter().zip(payloads.iter()).map(|(e, p)| {
        if e.is_dir || p.is_empty() { 0 } else {
            u64::from_le_bytes(p[0..8].try_into().unwrap_or([0;8]))
        }
    }).sum();

    println!("Archivio creato: {}", archive);
    println!("  {} entry", entries.len());
    if total_original > 0 {
        let ratio = 100.0 * total_compressed as f64 / total_original as f64;
        println!("  {:.1} KB -> {:.1} KB ({:.1}%)", total_original as f64/1024.0, total_compressed as f64/1024.0, ratio);
    }
    Ok(())
}

fn cmd_extract(archive: &str, dest: Option<&str>) -> io::Result<()> {
    let dest = Path::new(dest.unwrap_or("."));
    let mut f = File::open(archive)?;

    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "non è un archivio .piadina"));
    }
    let version = read_u8(&mut f)?;
    if version != VERSION {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("versione non supportata: {}", version)));
    }

    let n_entries = read_u32(&mut f)? as usize;
    let mut entries = Vec::with_capacity(n_entries);
    for _ in 0..n_entries {
        entries.push(read_entry_header(&mut f)?);
    }

    // read all payloads
    let mut all_payload = Vec::new();
    f.read_to_end(&mut all_payload)?;

    for e in &entries {
        let safe_path = sanitize_path(&e.path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData,
                format!("path non sicuro bloccato: {}", e.path)))?;
        let out_path = dest.join(&safe_path);

        if e.is_dir {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let start = e.data_offset as usize;
            let end   = start + e.data_len as usize;
            let blob  = &all_payload[start..end];
            let raw = decompress_bytes(blob)?;
            let mut out = File::create(&out_path)?;
            out.write_all(&raw)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out_path, fs::Permissions::from_mode(e.mode))?;
            }
        }
        println!("  estraendo {}", e.path);
    }
    println!("Estratto in {}", dest.display());
    Ok(())
}

fn cmd_list(archive: &str) -> io::Result<()> {
    let mut f = File::open(archive)?;
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "non è un archivio .piadina"));
    }
    let _version = read_u8(&mut f)?;
    let n_entries = read_u32(&mut f)? as usize;
    let mut entries = Vec::with_capacity(n_entries);
    for _ in 0..n_entries { entries.push(read_entry_header(&mut f)?); }

    let mut all_payload = Vec::new();
    f.read_to_end(&mut all_payload)?;

    println!("{:<6} {:>10} {:>10} {:>6}  {}", "tipo", "orig", "compr", "ratio", "path");
    println!("{}", "-".repeat(55));
    for e in &entries {
        if e.is_dir {
            println!("{:<6} {:>10} {:>10} {:>6}  {}", "dir", "-", "-", "-", e.path);
        } else {
            let start = e.data_offset as usize;
            let orig = if e.data_len >= 8 {
                u64::from_le_bytes(all_payload[start..start+8].try_into().unwrap_or([0;8]))
            } else { 0 };
            let ratio = if orig > 0 { format!("{:.0}%", 100.0 * e.data_len as f64 / orig as f64) } else { "-".into() };
            println!("{:<6} {:>10} {:>10} {:>6}  {}", "file",
                format!("{}", orig), format!("{}", e.data_len), ratio, e.path);
        }
    }
    Ok(())
}

fn cmd_info(archive: &str) -> io::Result<()> {
    let mut f = File::open(archive)?;
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "non è un archivio .piadina"));
    }
    let version = read_u8(&mut f)?;
    let n_entries = read_u32(&mut f)? as usize;
    let mut entries = Vec::with_capacity(n_entries);
    for _ in 0..n_entries { entries.push(read_entry_header(&mut f)?); }
    let mut all_payload = Vec::new();
    f.read_to_end(&mut all_payload)?;

    let n_files = entries.iter().filter(|e| !e.is_dir).count();
    let n_dirs  = entries.iter().filter(|e|  e.is_dir).count();
    let total_orig: u64 = entries.iter().filter(|e| !e.is_dir).map(|e| {
        let s = e.data_offset as usize;
        if e.data_len >= 8 { u64::from_le_bytes(all_payload[s..s+8].try_into().unwrap_or([0;8])) } else { 0 }
    }).sum();
    let total_comp: u64 = entries.iter().map(|e| e.data_len).sum();

    println!("Archivio : {}", archive);
    println!("Versione : {}", version);
    println!("File     : {}", n_files);
    println!("Cartelle : {}", n_dirs);
    println!("Originale: {} bytes", total_orig);
    println!("Compressa: {} bytes", total_comp);
    if total_orig > 0 {
        println!("Ratio    : {:.1}%", 100.0 * total_comp as f64 / total_orig as f64);
    }
    Ok(())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn usage() -> ! {
    eprintln!("Uso: piadazip <comando> [opzioni]

Comandi:
  create  <archivio.piadina> <file/dir>...   crea archivio
  extract <archivio.piadina> [-C <dest>]     estrae archivio
  list    <archivio.piadina>                 elenca contenuto
  info    <archivio.piadina>                 statistiche

Alias: c, x, l, t (come tar)

Esempi:
  piadazip create  backup.piadina src/ README.md
  piadazip extract backup.piadina -C ./output
  piadazip list    backup.piadina
  piadazip info    backup.piadina");
    std::process::exit(1);
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 { usage(); }

    match args[1].as_str() {
        "create" | "c" => {
            let archive = &args[2];
            let sources: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();
            if sources.is_empty() { usage(); }
            cmd_create(archive, &sources)
        }
        "extract" | "x" => {
            let archive = &args[2];
            let dest = if args.len() >= 5 && args[3] == "-C" { Some(args[4].as_str()) } else { None };
            cmd_extract(archive, dest)
        }
        "list" | "l" | "t" => cmd_list(&args[2]),
        "info"              => cmd_info(&args[2]),
        _ => usage(),
    }
}
