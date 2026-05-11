use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::{compress, crc32};

// ─── Costanti formato v3 ──────────────────────────────────────────────────────

pub const MAGIC: &[u8; 8] = b"PIADINA\0";
pub const VERSION: u8 = 3;

const FLAG_HAS_CHECKSUMS: u32 = 1 << 0;

// ─── Entry header ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EntryHeader {
    pub path: String,
    pub is_dir: bool,
    pub mode: u32,
    pub mtime: u64,
    pub data_offset: u64,
    pub data_len: u64,
    pub orig_len: u64,
    pub crc32: u32,
}


fn write_entry(w: &mut impl Write, e: &EntryHeader) -> io::Result<()> {
    let pb = e.path.as_bytes();
    w.write_all(&(pb.len() as u32).to_le_bytes())?;
    w.write_all(pb)?;
    w.write_all(&[e.is_dir as u8])?;
    w.write_all(&e.mode.to_le_bytes())?;
    w.write_all(&e.mtime.to_le_bytes())?;
    w.write_all(&e.data_offset.to_le_bytes())?;
    w.write_all(&e.data_len.to_le_bytes())?;
    w.write_all(&e.orig_len.to_le_bytes())?;
    w.write_all(&e.crc32.to_le_bytes())?;
    Ok(())
}

fn read_entry(r: &mut impl Read) -> io::Result<EntryHeader> {
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4)?;
    let plen = u32::from_le_bytes(b4) as usize;
    let mut pb = vec![0u8; plen];
    r.read_exact(&mut pb)?;
    let path = String::from_utf8(pb)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path non UTF-8"))?;

    let mut b1 = [0u8; 1];
    r.read_exact(&mut b1)?;
    let is_dir = b1[0] != 0;

    macro_rules! ru32 { () => {{ let mut b=[0u8;4]; r.read_exact(&mut b)?; u32::from_le_bytes(b) }} }
    macro_rules! ru64 { () => {{ let mut b=[0u8;8]; r.read_exact(&mut b)?; u64::from_le_bytes(b) }} }

    let mode        = ru32!();
    let mtime       = ru64!();
    let data_offset = ru64!();
    let data_len    = ru64!();
    let orig_len    = ru64!();
    let crc32       = ru32!();

    Ok(EntryHeader { path, is_dir, mode, mtime, data_offset, data_len, orig_len, crc32 })
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn collect_paths(src: &Path) -> io::Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    collect_recursive(src, src.parent().unwrap_or(Path::new("")), &mut out)?;
    Ok(out)
}

fn collect_recursive(current: &Path, base: &Path, out: &mut Vec<(PathBuf, String)>) -> io::Result<()> {
    let rel = current.strip_prefix(base).unwrap_or(current);
    let name = rel.to_string_lossy().into_owned();

    if current.is_dir() {
        out.push((current.to_path_buf(), name));
        for entry in fs::read_dir(current)? {
            collect_recursive(&entry?.path(), base, out)?;
        }
    } else {
        out.push((current.to_path_buf(), name));
    }
    Ok(())
}

fn sanitize(p: &str) -> Option<PathBuf> {
    let path = PathBuf::from(p);
    for c in path.components() {
        match c {
            std::path::Component::ParentDir | std::path::Component::RootDir => return None,
            _ => {}
        }
    }
    Some(path)
}

fn file_mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    { use std::os::unix::fs::MetadataExt; meta.mode() }
    #[cfg(not(unix))]
    { if meta.is_dir() { 0o755 } else { 0o644 } }
}

fn file_mtime(meta: &fs::Metadata) -> u64 {
    meta.modified().ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ─── create ──────────────────────────────────────────────────────────────────

pub fn cmd_create(
    archive_path: &str,
    sources: &[&str],
    cfg: compress::Config,
    verbose: bool,
) -> io::Result<()> {
    // Raccoglie tutti i file
    let mut all_paths: Vec<(PathBuf, String)> = Vec::new();
    for src in sources {
        let p = Path::new(src);
        if !p.exists() {
            eprintln!("warning: '{}' non trovato, saltato", src);
            continue;
        }
        all_paths.extend(collect_paths(p)?);
    }

    // Prima passata: comprimi tutto in memoria e raccoglie metadati
    struct FileEntry {
        header: EntryHeader,
        blob: Vec<u8>,
    }

    let mut entries: Vec<FileEntry> = Vec::new();
    let mut payload_offset: u64 = 0;

    for (disk_path, archive_name) in &all_paths {
        let meta = fs::metadata(disk_path)?;
        let mode  = file_mode(&meta);
        let mtime = file_mtime(&meta);

        if meta.is_dir() {
            entries.push(FileEntry {
                header: EntryHeader {
                    path: archive_name.clone(),
                    is_dir: true,
                    mode, mtime,
                    data_offset: 0,
                    data_len: 0,
                    orig_len: 0,
                    crc32: 0,
                },
                blob: Vec::new(),
            });
        } else {
            let raw = fs::read(disk_path)?;
            let checksum = crc32::crc32(&raw);
            let blob = compress::compress(&raw, &cfg)?;
            let blob_len = blob.len() as u64;
            let orig_len = raw.len() as u64;

            if verbose {
                let ratio = if orig_len > 0 { 100.0 * blob_len as f64 / orig_len as f64 } else { 0.0 };
                println!("  aggiungendo {} ({:.1}%)", archive_name, ratio);
            }

            entries.push(FileEntry {
                header: EntryHeader {
                    path: archive_name.clone(),
                    is_dir: false,
                    mode, mtime,
                    data_offset: payload_offset,
                    data_len: blob_len,
                    orig_len,
                    crc32: checksum,
                },
                blob,
            });
            payload_offset += blob_len;
        }
    }

    // Scrivi archivio
    let f = File::create(archive_path)?;
    let mut w = BufWriter::new(f);

    w.write_all(MAGIC)?;
    w.write_all(&[VERSION])?;
    w.write_all(&(entries.len() as u32).to_le_bytes())?;
    w.write_all(&FLAG_HAS_CHECKSUMS.to_le_bytes())?;

    for e in &entries {
        write_entry(&mut w, &e.header)?;
    }
    for e in &entries {
        w.write_all(&e.blob)?;
    }
    w.flush()?;

    let total_orig: u64 = entries.iter().map(|e| e.header.orig_len).sum();
    let total_comp: u64 = entries.iter().map(|e| e.header.data_len).sum();
    let n_files   = entries.iter().filter(|e| !e.header.is_dir).count();

    println!("Archivio creato: {}", archive_path);
    println!("  {} file, {} cartelle", n_files, entries.len() - n_files);
    if total_orig > 0 {
        println!("  {:.1} KB → {:.1} KB ({:.1}%)",
            total_orig as f64 / 1024.0,
            total_comp as f64 / 1024.0,
            100.0 * total_comp as f64 / total_orig as f64);
    }
    Ok(())
}

// ─── extract ─────────────────────────────────────────────────────────────────

fn open_archive(archive_path: &str) -> io::Result<(Vec<EntryHeader>, u64)> {
    let mut f = BufReader::new(File::open(archive_path)?);

    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "non è un archivio .piadina"));
    }
    let mut vb = [0u8; 1];
    f.read_exact(&mut vb)?;
    if vb[0] != VERSION {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("versione non supportata: {} (questo tool gestisce v{})", vb[0], VERSION)));
    }

    let mut b4 = [0u8; 4];
    f.read_exact(&mut b4)?;
    let n_entries = u32::from_le_bytes(b4) as usize;
    f.read_exact(&mut b4)?; // flags (ignorato per ora)

    let mut entries = Vec::with_capacity(n_entries);
    for _ in 0..n_entries {
        entries.push(read_entry(&mut f)?);
    }

    let payload_start = f.stream_position()?;
    Ok((entries, payload_start))
}

pub fn cmd_extract(archive_path: &str, dest: Option<&str>, verbose: bool) -> io::Result<()> {
    let dest = Path::new(dest.unwrap_or("."));
    let (entries, payload_start) = open_archive(archive_path)?;

    let mut f = File::open(archive_path)?;

    for e in &entries {
        let safe = sanitize(&e.path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData,
                format!("path non sicuro bloccato: {}", e.path)))?;
        let out_path = dest.join(&safe);

        if e.is_dir {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // seek + read del solo blob di questo file
            f.seek(SeekFrom::Start(payload_start + e.data_offset))?;
            let mut blob = vec![0u8; e.data_len as usize];
            f.read_exact(&mut blob)?;

            let raw = compress::decompress(&blob)?;

            // verifica CRC32
            let got_crc = crc32::crc32(&raw);
            if got_crc != e.crc32 {
                return Err(io::Error::new(io::ErrorKind::InvalidData,
                    format!("{}: CRC32 fallito (atteso {:08x}, ottenuto {:08x})",
                        e.path, e.crc32, got_crc)));
            }

            let mut out = File::create(&out_path)?;
            out.write_all(&raw)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out_path, fs::Permissions::from_mode(e.mode))?;
            }
        }
        if verbose {
            println!("  {}", e.path);
        }
    }
    println!("Estratto in: {}", dest.display());
    Ok(())
}

// ─── list ─────────────────────────────────────────────────────────────────────

pub fn cmd_list(archive_path: &str) -> io::Result<()> {
    let (entries, _) = open_archive(archive_path)?;

    println!("{:<6} {:>10} {:>10} {:>6}  {}", "tipo", "orig", "compr", "ratio", "path");
    println!("{}", "─".repeat(55));
    for e in &entries {
        if e.is_dir {
            println!("{:<6} {:>10} {:>10} {:>6}  {}", "dir", "─", "─", "─", e.path);
        } else {
            let ratio = if e.orig_len > 0 {
                format!("{:.0}%", 100.0 * e.data_len as f64 / e.orig_len as f64)
            } else { "─".into() };
            println!("{:<6} {:>10} {:>10} {:>6}  {}",
                "file", e.orig_len, e.data_len, ratio, e.path);
        }
    }
    Ok(())
}

// ─── info ─────────────────────────────────────────────────────────────────────

pub fn cmd_info(archive_path: &str) -> io::Result<()> {
    let (entries, _) = open_archive(archive_path)?;

    let n_files = entries.iter().filter(|e| !e.is_dir).count();
    let n_dirs  = entries.iter().filter(|e|  e.is_dir).count();
    let total_orig: u64 = entries.iter().map(|e| e.orig_len).sum();
    let total_comp: u64 = entries.iter().map(|e| e.data_len).sum();

    println!("Archivio : {}", archive_path);
    println!("Versione : {}", VERSION);
    println!("File     : {}", n_files);
    println!("Cartelle : {}", n_dirs);
    println!("Originale: {} bytes", total_orig);
    println!("Compressa: {} bytes", total_comp);
    if total_orig > 0 {
        println!("Ratio    : {:.1}%", 100.0 * total_comp as f64 / total_orig as f64);
    }
    Ok(())
}

// ─── test ─────────────────────────────────────────────────────────────────────

pub fn cmd_test(archive_path: &str) -> io::Result<()> {
    let (entries, payload_start) = open_archive(archive_path)?;
    let mut f = File::open(archive_path)?;
    let mut errors = 0u32;

    for e in &entries {
        if e.is_dir { continue; }

        f.seek(SeekFrom::Start(payload_start + e.data_offset))?;
        let mut blob = vec![0u8; e.data_len as usize];
        f.read_exact(&mut blob)?;

        match compress::decompress(&blob) {
            Err(err) => {
                eprintln!("FAIL {}: {}", e.path, err);
                errors += 1;
            }
            Ok(raw) => {
                let got = crc32::crc32(&raw);
                if got != e.crc32 {
                    eprintln!("FAIL {}: CRC32 {:08x} atteso {:08x}", e.path, e.crc32, got);
                    errors += 1;
                } else {
                    println!("  OK  {}", e.path);
                }
            }
        }
    }

    if errors > 0 {
        Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("{} file con errori su {}", errors, entries.iter().filter(|e| !e.is_dir).count())))
    } else {
        println!("Archivio integro.");
        Ok(())
    }
}

// ─── compress / decompress singolo file ──────────────────────────────────────

pub fn cmd_compress_file(path: &str, level: u8) -> io::Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("'{}' non trovato", path)));
    }
    let sources = [path];
    let out = format!("{}.piadina", path);
    cmd_create(&out, &sources.iter().map(|s| *s).collect::<Vec<_>>(), compress::Config::new(level), false)?;
    println!("→ {}", out);
    Ok(())
}

pub fn cmd_decompress_file(path: &str, dest: Option<&str>) -> io::Result<()> {
    cmd_extract(path, dest, false)
}
