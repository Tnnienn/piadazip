# PiadaZip

Archiver e compressore a linea di comando con formato `.piadina`.

Compressione in due stadi: **LZSS** (finestra scorrevole fino a 32 KB) seguito da **codifica di Huffman** adattiva — ratio paragonabili a gzip, zero dipendenze esterne.

---

## Installazione

### Da sorgente (richiede Rust ≥ 1.85)

```bash
cargo build --release
sudo install -m755 target/release/piadazip /usr/local/bin/
```

### Man page (opzionale)

```bash
sudo install -m644 man/piadazip.1 /usr/local/share/man/man1/
sudo mandb
man piadazip
```

---

## Uso rapido

```bash
# Crea archivio da file e cartelle
piadazip create backup.piadina src/ README.md --level 6

# Elenca contenuto
piadazip list backup.piadina

# Verifica integrità (CRC32)
piadazip test backup.piadina

# Estrai
piadazip extract backup.piadina -C ./ripristino

# Statistiche
piadazip info backup.piadina

# Singolo file (come gzip)
piadazip compress documento.txt --level 9
piadazip decompress documento.txt.piadina
```

---

## Comandi

| Comando       | Alias | Descrizione |
|---------------|-------|-------------|
| `create`      | `c`   | Crea archivio da file/cartelle |
| `extract`     | `x`   | Estrae archivio |
| `list`        | `l`, `t` | Elenca contenuto con ratio |
| `info`        |       | Statistiche riassuntive |
| `test`        |       | Verifica CRC32 senza estrarre |
| `compress`    |       | Comprimi singolo file → `.piadina` |
| `decompress`  |       | Decomprimi singolo file |

## Opzioni

| Opzione | Descrizione |
|---------|-------------|
| `--level N` / `-N` | Livello compressione 1–9 (default: 6) |
| `--verbose` / `-v` | Output dettagliato per file |
| `-C <dir>` | Directory di destinazione per extract |
| `--help` / `-h` | Aiuto (anche per sotto-comandi) |

---

## Livelli di compressione

| Livello | Finestra | Descrizione |
|---------|----------|-------------|
| 1       | 4 KB     | Velocissimo, ratio base |
| 1–4     |  4–16 KB | Bilanciato velocità/ratio |
| 5–6     | 16–32 KB | Modalità default — buon compromesso |
| 7–9     | 32 KB    | Lazy matching attivo, massima compressione |

---

## Formato `.piadina` (v3)

```
[0..8]   Magic       = "PIADINA\0"
[8]      VERSION     = 3
[9..13]  N_ENTRIES   (u32 LE)
[13..17] FLAGS       (u32 LE) — bit0: checksum abilitati
[17+]    ENTRY INDEX — per ogni entry:
           path_len(u32) + path(UTF-8)
           is_dir(u8) + mode(u32) + mtime(u64)
           data_offset(u64) + data_len(u64) + orig_len(u64)
           crc32(u32)
[...]    PAYLOAD — blob compressi concatenati
```

Ogni blob usa la pipeline **LZSS → Huffman**. File già compressi (JPEG, PNG, ZIP) vengono archiviati in modalità store quando il ratio supererebbe il 95%. Gli offset nell'indice permettono l'estrazione di un singolo file senza caricare l'intero archivio in memoria.

---

## Algoritmo

**LZSS** scansiona l'input con una finestra scorrevole e una hash chain per trovare rapidamente le stringhe ripetute. Ogni match viene codificato come tripletta `(offset, lunghezza)` invece di riscrivere i byte. Il token stream risultante viene poi compresso con **Huffman adattivo** (two-pass: primo passaggio per le frequenze, secondo per la codifica).

Rispetto a RLE+Huffman (v2), LZSS cattura la ridondanza a lungo raggio ottenendo tipicamente il 30–60% di compressione in più su testo strutturato e codice sorgente.
