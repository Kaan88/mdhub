//! What the window shows and what a pointer can hit. The drawing code (`view.rs`, `grid.rs`,
//! `ui.rs`) reads a `Vm` and returns a `Layout`; the shell (`app.rs`) builds the `Vm` from
//! hub-core and turns a `Hit` into work. Neither side owns the other's files.
use hub_core::model::Row;
use mdshared_ui::gfx::Rect;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum View {
    #[default]
    List,
    Large,
    Small,
}

#[derive(Clone, Debug)]
pub struct AppVm {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub glyph: &'static str,
    pub tile: [u8; 3],
    pub row: Row,
    /// Installed version text, or None.
    pub installed: Option<String>,
    /// Latest version text, or None when unknown.
    pub latest: Option<String>,
    /// "21 Nov" or empty.
    pub date: String,
    /// "35.2 MB" or empty: what an install or update downloads.
    pub size: String,
    pub running: bool,
    /// One-line release notes (hub.txt `notes`).
    pub notes: String,
    /// Detail lines for the pane under the list: (label, value).
    pub details: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Banner {
    pub text: String,
    pub yes: String,
    pub no: String,
    pub warn: bool,
    /// The bar is Hub's own ("Restart now"), not an app's question: a click on it is a
    /// different command, so a second click of a double-click cannot land on the other bar.
    pub hub: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Vm {
    pub apps: Vec<AppVm>,
    pub sel: usize,
    pub hover: Option<usize>,
    pub view: View,
    pub banner: Option<Banner>,
    /// "Checked 4 minutes ago", "Checking…", "Last check failed: no connection".
    pub checked: String,
    pub hub_version: String,
    /// Some(text) while an install runs: the bar shows Cancel and this text.
    pub busy: Option<String>,
    /// How many rows have `Row::Update`.
    pub updates: usize,
    /// Filter text of the apps filter box.
    pub filter: String,
    pub filter_focus: bool,
    /// A picture of the settings card over the window (`--demo settings-...`): its rows and the
    /// category shown. The real window keeps a form of its own, so this is None there.
    pub settings: Option<(Vec<mdshared_ui::settings::Row>, usize)>,
    /// The release notes view: when it is Some, it takes the content area (the list and the pane
    /// are not drawn) until it is closed.
    pub notes: Option<NotesVm>,
}

/// Where the full release notes of the version Hub knows about are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotesState {
    /// The notes file is being fetched.
    Loading,
    /// The notes, cleaned Markdown.
    Loaded(String),
    /// The fetch failed: the reason. "Try again" asks once more.
    Failed(String),
    /// The release names no notes file: the one line is all there is.
    NoFile,
}

/// The release notes view of one app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotesVm {
    pub id: &'static str,
    pub name: &'static str,
    /// The version the notes belong to (the latest Hub knows about).
    pub version: String,
    /// The installed version text, or None.
    pub installed: Option<String>,
    /// "21 Nov" or empty.
    pub date: String,
    /// hub.txt `notes`: always there when the release has any.
    pub one_liner: String,
    pub state: NotesState,
}

/// What a row or tile's button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Open,
    Install,
    Update,
    Adopt,
    Cancel,
    TryAgain,
}

/// What a point on the window is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// Title row empty space: drag the window.
    Drag,
    Min,
    Max,
    Close,
    UpdateAll,
    CheckNow,
    CancelAll,
    ViewSwitch(View),
    Filter,
    /// The sliders button: the settings card. The shell's own, like the window buttons.
    Settings,
    /// The row's or tile's body.
    Row(usize),
    /// The row's or tile's button.
    Action(usize, Act),
    /// The row's "..." menu button.
    Menu(usize),
    BannerYes,
    BannerNo,
    /// The pane's "Release notes" link of row `i`.
    Notes(usize),
    /// The notes view's back arrow.
    NotesBack,
    /// The notes view's "Try again".
    NotesRetry,
    /// The notes view's "GitHub": the release page in the browser.
    NotesPage,
    None,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Hit rectangles, front-most last; the shell tests them in reverse.
    pub hits: Vec<(Rect, Hit)>,
    /// Content height of the scrolling list, so the shell can scroll it.
    pub list_content_h: f32,
    /// The notes view: how far its text can scroll and how tall its window on it is (both 0 when
    /// the view is not open), so the shell can clamp the wheel and move by pages.
    pub notes_max_scroll: f32,
    pub notes_view_h: f32,
}

impl Layout {
    pub fn at(&self, x: f32, y: f32) -> Hit {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, h)| *h)
            .unwrap_or(Hit::None)
    }
}
