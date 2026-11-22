//! Types every module and the window share. Change a signature here only with the owner of the
//! other side in mind: the window (bins/hub) reads these.
use mdshared_app::update::Version;
use std::path::PathBuf;

/// How an app was found on this PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Hub's own record (`installed.ini`).
    Hub,
    /// An uninstall entry (HKCU/HKLM `...\Uninstall`).
    Uninstall,
    /// A folder installers use (`%LOCALAPPDATA%\Programs\<folder>`, Program Files).
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub folder: PathBuf,
    /// Absolute path of the executable Hub starts.
    pub exe: PathBuf,
    pub version: Option<Version>,
    pub source: Source,
    /// "setup" | "files" | "zip": how it got here, when Hub did it; empty otherwise.
    pub kind: String,
}

/// The kind of install a manifest asks for on a platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Setup,
    Files,
    Zip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    Machine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSpec {
    /// A plain file name (letters, digits, `.`, `_`, `-`).
    pub name: String,
    pub url: String,
    /// Lower-case 64-digit hex.
    pub sha256: String,
    /// Bytes; 0 when the source does not say (a legacy manifest).
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupSpec {
    pub file: FileSpec,
    /// Arguments for a silent run, from a restricted charset.
    pub silent: String,
    pub scope: Scope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformEntry {
    /// `windows-x86_64` ...
    pub platform: String,
    pub kind: Kind,
    /// Relative to the install folder.
    pub exe: Option<String>,
    pub files: Vec<FileSpec>,
    pub zip: Option<FileSpec>,
    pub setup: Option<SetupSpec>,
    pub uninstall: Option<String>,
    pub min: Option<Version>,
    pub wait: Vec<String>,
    pub updates_self: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubManifest {
    pub app: String,
    pub version: Version,
    pub date: String,
    pub notes: String,
    pub notes_file: Option<String>,
    pub platforms: Vec<PlatformEntry>,
}

/// What a check learned about one app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Latest {
    /// Not asked yet (or never succeeded).
    Unknown,
    /// The release carries no manifest Hub can read (yet); the reason.
    NoManifest(String),
    Found(Box<HubManifest>),
}

/// One row's state, for the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    NotInstalled,
    UpToDate,
    Update,
    /// Found by detection, not recorded by Hub: offered for Adopt.
    External,
    Downloading(f32, String),
    Verifying,
    Waiting(String),
    Failed(String),
    Done(String),
    /// Installed, but no release information Hub can use.
    NoInfo(String),
}
