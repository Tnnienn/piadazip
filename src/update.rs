use std::io;
use std::path::Path;
use std::process::Command;

const REPO: &str = "Tnnienn/piadazip";

pub fn cmd_update() -> io::Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    eprint!("Controllo aggiornamenti...");
    let latest_tag = fetch_latest_tag()?;
    let latest = latest_tag.trim_start_matches('v');
    eprintln!(" ok");

    if !is_newer(latest, current) {
        println!("piadazip {} è già aggiornato.", current);
        return Ok(());
    }

    println!("Nuova versione disponibile: {} → {}", current, latest_tag);

    let bin_name = release_binary_name();
    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, latest_tag, bin_name
    );

    let exe = std::env::current_exe()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("current_exe: {}", e)))?;

    let tmp = std::env::temp_dir().join(format!("piadazip-{}", latest_tag));

    eprint!("Download {}...", bin_name);
    curl_download(&url, &tmp)?;
    eprintln!(" ok");

    replace_exe(&tmp, &exe)?;

    println!("piadazip aggiornato a {}.", latest_tag);
    Ok(())
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

fn curl_get(url: &str) -> io::Result<String> {
    let out = Command::new("curl")
        .args(["-fsSL", "-A", "piadazip-updater", url])
        .output()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound,
            format!("curl non trovato. Aggiorna manualmente: https://github.com/{}/releases", REPO)))?;
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other,
            format!("curl ha restituito codice {}", out.status)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn curl_download(url: &str, dest: &Path) -> io::Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "-A", "piadazip-updater", "--progress-bar", url, "-o"])
        .arg(dest)
        .status()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "curl non trovato"))?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other,
            "download fallito — release non trovata per questa piattaforma?"));
    }
    Ok(())
}

// ── GitHub release ────────────────────────────────────────────────────────────

fn fetch_latest_tag() -> io::Result<String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let body = curl_get(&url)?;
    parse_tag_name(&body).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "tag_name non trovato nella risposta GitHub")
    })
}

fn parse_tag_name(json: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let start = json.find(key)?;
    let after = &json[start + key.len()..];
    let colon = after.find(':')?;
    let quoted = after[colon + 1..].trim_start();
    if quoted.starts_with('"') {
        let inner = &quoted[1..];
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        None
    }
}

fn release_binary_name() -> String {
    let os = match std::env::consts::OS {
        "linux"   => "linux",
        "macos"   => "macos",
        "windows" => "windows",
        other     => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64"  => "x86_64",
        "aarch64" => "aarch64",
        other     => other,
    };
    if cfg!(windows) {
        format!("piadazip-{}-{}.exe", os, arch)
    } else {
        format!("piadazip-{}-{}", os, arch)
    }
}

// ── Versione ──────────────────────────────────────────────────────────────────

fn is_newer(latest: &str, current: &str) -> bool {
    fn parse(v: &str) -> (u32, u32, u32) {
        let mut p = v.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
        (p.next().unwrap_or(0), p.next().unwrap_or(0), p.next().unwrap_or(0))
    }
    parse(latest) > parse(current)
}

// ── Sostituzione eseguibile ───────────────────────────────────────────────────

fn replace_exe(src: &Path, dest: &Path) -> io::Result<()> {
    replace_platform(src, dest)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

#[cfg(unix)]
fn replace_platform(src: &Path, dest: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::copy(src, dest).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            io::Error::new(e.kind(), "permessi insufficienti. Riprova con: sudo piadazip update")
        } else {
            e
        }
    })?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))
}

#[cfg(windows)]
fn replace_platform(src: &Path, dest: &Path) -> io::Result<()> {
    // Su Windows non si può sovrascrivere un exe in esecuzione:
    // rinomina il vecchio in .old, copia il nuovo, poi elimina il .old.
    let old = dest.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(dest, &old)?;
    if let Err(e) = std::fs::copy(src, dest) {
        let _ = std::fs::rename(&old, dest); // rollback
        return Err(e);
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace_platform(src: &Path, dest: &Path) -> io::Result<()> {
    std::fs::copy(src, dest)?;
    Ok(())
}
