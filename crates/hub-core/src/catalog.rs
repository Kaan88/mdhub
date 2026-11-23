//! The apps Hub knows: compiled in. Adding one is a new entry and a release of Hub.
//! Scope of this slice, by the owner: mdedit, mdview, Sprawl, Inbound.

pub struct CatalogApp {
    /// Lower-case id; the `app:` of its hub.txt.
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Repository under `Kaan88`.
    pub repo: &'static str,
    /// Executable Hub starts, relative to the install folder.
    pub exe: &'static str,
    /// Arguments Hub starts it with.
    pub launch_args: &'static [&'static str],
    /// Folder under `%LOCALAPPDATA%\Programs` (per user installs).
    pub folder: &'static str,
    /// Program Files folder name for a machine install (`Sprawl`), if any.
    pub machine_folder: Option<&'static str>,
    /// Inno `AppId` uninstall subkeys (`{GUID}_is1`, `Sprawl_is1`).
    pub uninstall_keys: &'static [&'static str],
    /// Executables (relative) that must be gone before an install, besides `exe`.
    pub wait: &'static [&'static str],
    /// The app's own setup stops the running app (Inno `CloseApplications`, or a stop script), as
    /// double-clicking it would. For such an app Hub asks visible windows to close, then goes on
    /// and runs the setup instead of refusing with "still open" (D-029). Only meaningful for a
    /// setup install; a `files` install has no installer to do it and stays strict.
    pub setup_closes_app: bool,
    /// The format-1/2 manifest file of the release that mdedit and mdview publish; a stopgap
    /// until their release carries hub.txt. None for apps that have no such file.
    pub legacy_manifest: Option<&'static str>,
    /// Tile colour and glyph name for the window.
    pub tile: [u8; 3],
    pub glyph: &'static str,
}

pub const CATALOG: [CatalogApp; 4] = [
    CatalogApp {
        id: "mdedit",
        name: "mdedit",
        blurb: "Light text editor with syntax highlighting",
        repo: "markdown-editor-rs",
        exe: "mdedit.exe",
        launch_args: &[],
        folder: "mdedit",
        machine_folder: None,
        uninstall_keys: &["{C58A9AB1-8337-4056-9953-04E92C05F414}_is1"],
        wait: &["mdedit-ai.exe", "mdview-engine.exe"],
        setup_closes_app: false,
        legacy_manifest: Some("mdedit-update-2.txt"),
        tile: [0x2f, 0x6f, 0xd6],
        glyph: "edit",
    },
    CatalogApp {
        id: "mdview",
        name: "mdview",
        blurb: "Markdown viewer",
        repo: "markdown-viewer-rs",
        exe: "mdview.exe",
        launch_args: &[],
        folder: "mdview",
        machine_folder: None,
        uninstall_keys: &["{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}_is1"],
        wait: &["mdview-engine.exe"],
        setup_closes_app: false,
        legacy_manifest: Some("mdview-update.txt"),
        tile: [0x1d, 0x7d, 0x6b],
        glyph: "view",
    },
    CatalogApp {
        id: "sprawl",
        name: "Sprawl",
        blurb: "Disk space analyzer and cleaner",
        repo: "sprawl",
        exe: "sprawl.exe",
        launch_args: &[],
        folder: "Sprawl",
        machine_folder: Some("Sprawl"),
        uninstall_keys: &["Sprawl_is1"],
        wait: &[],
        setup_closes_app: true,
        legacy_manifest: None,
        tile: [0xc0, 0x6a, 0x1a],
        glyph: "folder",
    },
    CatalogApp {
        id: "inbound",
        name: "Inbound",
        blurb: "Download manager: HTTP, torrents, remote files",
        repo: "inbound",
        exe: "app\\DownloadManager.Host.exe",
        launch_args: &["--open"],
        folder: "Inbound Download Manager",
        machine_folder: None,
        uninstall_keys: &["{6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}_is1"],
        wait: &[
            "app\\DownloadManager.Host.exe",
            "app\\DownloadManager.Desktop.WinUI.exe",
            "app\\DownloadManager.NativeHost.exe",
            "app\\DownloadManager.Server.exe",
        ],
        setup_closes_app: true,
        legacy_manifest: None,
        tile: [0x1d, 0x8a, 0x9e],
        glyph: "download",
    },
];

pub fn find(id: &str) -> Option<&'static CatalogApp> {
    CATALOG.iter().find(|a| a.id == id)
}

impl CatalogApp {
    /// `https://github.com/Kaan88/<repo>/releases/latest/download/<file>`
    pub fn latest_url(&self, file: &str) -> String {
        format!(
            "https://github.com/Kaan88/{}/releases/latest/download/{file}",
            self.repo
        )
    }
    /// A URL a manifest of this app may name: under its own repository's releases only.
    pub fn url_allowed(&self, url: &str) -> bool {
        url.starts_with(&format!(
            "https://github.com/Kaan88/{}/releases/download/",
            self.repo
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_whose_own_setup_stops_them_are_flagged() {
        let flagged: Vec<&str> = CATALOG
            .iter()
            .filter(|a| a.setup_closes_app)
            .map(|a| a.id)
            .collect();
        assert_eq!(flagged, ["sprawl", "inbound"]);
    }

    #[test]
    fn inbounds_wait_list_has_every_program_its_stop_script_covers_that_hub_can_place() {
        let inbound = find("inbound").unwrap();
        for exe in [
            r"app\DownloadManager.Host.exe",
            r"app\DownloadManager.Desktop.WinUI.exe",
            r"app\DownloadManager.NativeHost.exe",
            r"app\DownloadManager.Server.exe",
        ] {
            assert!(inbound.wait.contains(&exe), "{exe}");
        }
    }
}
