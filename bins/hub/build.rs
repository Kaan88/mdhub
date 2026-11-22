//! Embeds the icon and a VERSIONINFO block (Hub's own detection and Windows' Details tab read the
//! version from it) on Windows, with the SDK's `rc.exe`. Without one the build still succeeds; the
//! exe just has no icon and no version resource.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/hub.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe not found; hub.exe is built without its icon and version");
        return;
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../assets/hub.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let numeric: Vec<u32> = version
        .split(['-', '+'])
        .next()
        .unwrap_or("0.0.0")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let (a, b, c) = (
        numeric.first().copied().unwrap_or(0),
        numeric.get(1).copied().unwrap_or(0),
        numeric.get(2).copied().unwrap_or(0),
    );
    let script = out.join("hub.rc");
    let res = out.join("hub.res");
    let text = format!(
        "1 ICON \"{icon}\"\n\
         1 VERSIONINFO\n FILEVERSION {a},{b},{c},0\n PRODUCTVERSION {a},{b},{c},0\n FILEOS 0x40004\n FILETYPE 1\n\
         BEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n BLOCK \"040904B0\"\n BEGIN\n\
         VALUE \"CompanyName\", \"Kaan88\\0\"\n VALUE \"FileDescription\", \"Hub: launcher and updater\\0\"\n\
         VALUE \"FileVersion\", \"{version}\\0\"\n VALUE \"InternalName\", \"hub\\0\"\n\
         VALUE \"OriginalFilename\", \"hub.exe\\0\"\n VALUE \"ProductName\", \"Hub\\0\"\n\
         VALUE \"ProductVersion\", \"{version}\\0\"\n END\n END\n\
         BLOCK \"VarFileInfo\"\n BEGIN\n VALUE \"Translation\", 0x0409, 1200\n END\n END\n",
        icon = icon.display().to_string().replace('\\', "/")
    );
    if std::fs::write(&script, text).is_err() {
        return;
    }
    let ok = Command::new(rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&script)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=the resource compiler failed; hub.exe has no icon or version");
    }
}

fn find_rc() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    if let Some(p) = std::env::split_paths(&paths)
        .map(|d| d.join("rc.exe"))
        .find(|p| p.is_file())
    {
        return Some(p);
    }
    let base = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut found: Vec<PathBuf> = std::fs::read_dir(base)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}
