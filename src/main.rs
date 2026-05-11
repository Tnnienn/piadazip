mod archive;
mod compress;
mod crc32;
mod huffman;
mod lzss;
mod update;

use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Help ─────────────────────────────────────────────────────────────────────

fn help_global() {
    println!("piadazip {} — archiver e compressore con formato .piadina\n", VERSION);
    println!("USO:");
    println!("  piadazip <comando> [opzioni]\n");
    println!("COMANDI:");
    println!("  create      c    Crea un archivio da file e cartelle");
    println!("  extract     x    Estrae un archivio");
    println!("  list        l,t  Elenca il contenuto");
    println!("  info             Mostra statistiche dell'archivio");
    println!("  test             Verifica l'integrità (CRC32)");
    println!("  compress         Comprimi un singolo file → file.piadina");
    println!("  decompress       Decomprimi un singolo file");
    println!("  update           Aggiorna piadazip all'ultima versione\n");
    println!("  piadazip <comando> --help   Aiuto specifico per un comando\n");
    println!("ESEMPI:");
    println!("  piadazip create backup.piadina src/ README.md --level 6");
    println!("  piadazip extract backup.piadina -C ./ripristino");
    println!("  piadazip list backup.piadina");
    println!("  piadazip test backup.piadina");
    println!("  piadazip compress documento.txt --level 9");
    println!("  piadazip decompress documento.txt.piadina");
}

fn help_create() {
    println!("piadazip create — crea un archivio .piadina\n");
    println!("USO:");
    println!("  piadazip create <archivio.piadina> <file|dir>... [opzioni]\n");
    println!("OPZIONI:");
    println!("  --level, -1..-9   Livello di compressione 1 (veloce) - 9 (migliore) [default: 6]");
    println!("  --verbose, -v     Mostra ogni file aggiunto con il ratio\n");
    println!("ESEMPI:");
    println!("  piadazip create arch.piadina src/ README.md");
    println!("  piadazip create arch.piadina progetto/ --level 9 -v");
    println!("  piadazip c arch.piadina file.txt  # alias corto");
}

fn help_extract() {
    println!("piadazip extract — estrae un archivio .piadina\n");
    println!("USO:");
    println!("  piadazip extract <archivio.piadina> [opzioni]\n");
    println!("OPZIONI:");
    println!("  -C <dest>         Directory di destinazione [default: directory corrente]");
    println!("  --verbose, -v     Mostra ogni file estratto\n");
    println!("ESEMPI:");
    println!("  piadazip extract arch.piadina");
    println!("  piadazip extract arch.piadina -C /tmp/out -v");
    println!("  piadazip x arch.piadina -C ./ripristino  # alias corto");
}

fn help_compress() {
    println!("piadazip compress — comprimi un singolo file\n");
    println!("USO:");
    println!("  piadazip compress <file> [--level 1-9]\n");
    println!("  Output: <file>.piadina nella stessa directory\n");
    println!("OPZIONI:");
    println!("  --level, -1..-9   Livello di compressione [default: 6]\n");
    println!("ESEMPI:");
    println!("  piadazip compress documento.txt");
    println!("  piadazip compress database.sql --level 9");
}

fn help_decompress() {
    println!("piadazip decompress — decomprimi un archivio mono-file\n");
    println!("USO:");
    println!("  piadazip decompress <archivio.piadina> [-C <dest>]\n");
    println!("ESEMPI:");
    println!("  piadazip decompress documento.txt.piadina");
    println!("  piadazip decompress documento.txt.piadina -C /tmp");
}

// ─── Arg parsing helpers ──────────────────────────────────────────────────────

fn parse_level(args: &[String]) -> u8 {
    for i in 0..args.len() {
        if args[i] == "--level" {
            if let Some(v) = args.get(i + 1) {
                if let Ok(n) = v.parse::<u8>() {
                    return n.clamp(1, 9);
                }
            }
        }
        // forma -N (es. -6, -9)
        if args[i].len() == 2 && args[i].starts_with('-') {
            if let Ok(n) = args[i][1..].parse::<u8>() {
                if (1..=9).contains(&n) { return n; }
            }
        }
    }
    6 // default
}

fn has_flag(args: &[String], short: &str, long: &str) -> bool {
    args.iter().any(|a| a == short || a == long)
}

fn flag_value<'a>(args: &'a [String], long: &str) -> Option<&'a str> {
    for i in 0..args.len() {
        if args[i] == long {
            return args.get(i + 1).map(|s| s.as_str());
        }
    }
    None
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn run() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        help_global();
        return Ok(());
    }

    let cmd = args[1].as_str();
    let rest = &args[2..];

    // help per sotto-comando
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        match cmd {
            "create" | "c"     => help_create(),
            "extract" | "x"    => help_extract(),
            "compress"         => help_compress(),
            "decompress"       => help_decompress(),
            _                  => help_global(),
        }
        return Ok(());
    }

    match cmd {
        "create" | "c" => {
            if rest.is_empty() { help_create(); return Ok(()); }
            let archive = &rest[0];
            // raccoglie sorgenti saltando flag e i loro valori
            let mut sources: Vec<&str> = Vec::new();
            let mut skip_next = false;
            for a in &rest[1..] {
                if skip_next { skip_next = false; continue; }
                if a == "--level" { skip_next = true; continue; }
                if a.starts_with('-') { continue; }
                sources.push(a.as_str());
            }
            if sources.is_empty() {
                eprintln!("Errore: nessun file sorgente specificato.");
                help_create();
                process::exit(1);
            }
            let level   = parse_level(rest);
            let verbose = has_flag(rest, "-v", "--verbose");
            archive::cmd_create(archive, &sources, compress::Config::new(level), verbose)?;
        }

        "extract" | "x" => {
            if rest.is_empty() { help_extract(); return Ok(()); }
            let archive = &rest[0];
            let dest    = flag_value(rest, "-C");
            let verbose = has_flag(rest, "-v", "--verbose");
            archive::cmd_extract(archive, dest, verbose)?;
        }

        "list" | "l" | "t" => {
            if rest.is_empty() { eprintln!("Errore: specifica l'archivio."); process::exit(1); }
            archive::cmd_list(&rest[0])?;
        }

        "info" => {
            if rest.is_empty() { eprintln!("Errore: specifica l'archivio."); process::exit(1); }
            archive::cmd_info(&rest[0])?;
        }

        "test" => {
            if rest.is_empty() { eprintln!("Errore: specifica l'archivio."); process::exit(1); }
            archive::cmd_test(&rest[0])?;
        }

        "compress" => {
            if rest.is_empty() { help_compress(); return Ok(()); }
            let file  = &rest[0];
            let level = parse_level(rest);
            archive::cmd_compress_file(file, level)?;
        }

        "decompress" => {
            if rest.is_empty() { help_decompress(); return Ok(()); }
            let file = &rest[0];
            let dest = flag_value(rest, "-C");
            archive::cmd_decompress_file(file, dest)?;
        }

        "update" => {
            update::cmd_update()?;
        }

        _ => {
            eprintln!("Comando sconosciuto: '{}'. Usa --help per la lista.", cmd);
            process::exit(1);
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Errore: {}", e);
        process::exit(1);
    }
}
