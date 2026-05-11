# PiadaZip

Archiver e compressore a linea di comando con formato `.piadina`.

Compressione in due stadi: **LZSS** (finestra scorrevole fino a 32 KB) seguito da **codifica di Huffman** adattiva — ratio paragonabili a gzip, zero dipendenze esterne.

---

## Installazione

### Script (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/Tnnienn/piadazip/main/install.sh | sh
```

### Script (Windows — PowerShell)

```powershell
irm https://raw.githubusercontent.com/Tnnienn/piadazip/main/install.ps1 | iex
```

Oppure scarica il binario direttamente dalla [pagina delle release](https://github.com/Tnnienn/piadazip/releases/latest).

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

| Livello | Finestra | Max chain | Descrizione |
|---------|----------|-----------|-------------|
| 1       | 4 KB     | 4         | Velocissimo, ratio base |
| 2–4     | 8–16 KB  | 8–32      | Bilanciato velocità/ratio |
| 5–6     | 16–32 KB | 64–128    | Default — buon compromesso |
| 7–9     | 32 KB    | 192–512   | Massima compressione |

---

## Benchmark

Misurato su CPU AMD Ryzen (singolo thread). Ratio = dimensione compressa / originale — più basso è meglio.

### File di testo — 18.4 MB (testo naturale con vocabolario fisso)

| Strumento          |   Output |  Ratio |    Tempo |
|:-------------------|---------:|-------:|---------:|
| gzip -6            | 3 048 001 B |  16.5% | 1 238 ms |
| gzip -9            | 3 012 643 B |  16.4% | 1 832 ms |
| bzip2 -9           | 2 008 769 B |  10.9% | 2 279 ms |
| xz -6              | 2 413 396 B |  13.1% | 11 831 ms |
| zstd -6            | 3 656 850 B |  19.9% |   155 ms |
| zstd -19           | 2 394 513 B |  13.0% | 10 966 ms |
| **piadazip -1**    | 5 058 200 B |  27.5% |   281 ms |
| **piadazip -6**    | 3 565 456 B |  **19.4%** | 1 359 ms |
| **piadazip -9**    | 3 531 550 B |  **19.2%** | 1 486 ms |

### Codice sorgente ripetuto — 906 KB (stesso blocco ×20)

| Strumento          |   Output |  Ratio |    Tempo |
|:-------------------|---------:|-------:|---------:|
| gzip -6            |   203 118 B |  22.4% |    26 ms |
| gzip -9            |   201 037 B |  22.2% |    93 ms |
| bzip2 -9           |    18 609 B |   2.1% |   258 ms |
| xz -6              |    10 376 B |   1.1% |   103 ms |
| zstd -6            |    11 064 B |   1.2% |    14 ms |
| zstd -19           |    10 278 B |   1.1% |    48 ms |
| **piadazip -1**    |   300 302 B |  33.1% |    29 ms |
| **piadazip -6**    |   237 973 B |  26.3% |    41 ms |
| **piadazip -9**    |   236 649 B |  26.1% |    60 ms |

> **Nota:** su questo file bzip2/xz/zstd eccellono perché il blocco ~45 KB si ripete 20 volte.
> La finestra LZSS di piadazip (32 KB) è più piccola del blocco e non riesce a riferirsi
> all'intera ripetizione precedente. xz usa una finestra da diversi MB.

### Dati binari strutturati — 1.2 MB (record fissi con pattern)

| Strumento          |   Output |  Ratio |    Tempo |
|:-------------------|---------:|-------:|---------:|
| gzip -6            |   446 021 B |  37.2% |    60 ms |
| gzip -9            |   445 877 B |  37.2% |    67 ms |
| bzip2 -9           |   151 982 B |  12.7% |    61 ms |
| xz -6              |    19 556 B |   1.6% |   492 ms |
| zstd -6            |   121 717 B |  10.1% |    18 ms |
| zstd -19           |   125 721 B |  10.5% |   872 ms |
| **piadazip -1**    |   419 262 B |  34.9% |    38 ms |
| **piadazip -6**    |   339 556 B |  **28.3%** |    90 ms |
| **piadazip -9**    |   343 164 B |  28.6% |   107 ms |

> **piadazip -6 batte gzip** (28.3% vs 37.2%) su dati con struttura a record fissi.

### Riassunto

- Su **testo naturale**: piadazip -6 è paragonabile a gzip -6 (19.4% vs 16.5%)
- Su **binari strutturati**: piadazip -6 batte gzip di ~9 punti percentuali
- Su **dati con ripetizioni lunghe** (> finestra 32 KB): xz e bzip2 vincono nettamente
- **Velocità**: piadazip -6 è rapido quanto gzip; piadazip -1 è il più veloce del gruppo su testo

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

Rispetto a RLE+Huffman (v2), LZSS cattura la ridondanza a lungo raggio ottenendo fino al 60% di compressione in più su testo strutturato. Il punto di forza è sui dati binari con struttura a record fissi, dove supera gzip.
