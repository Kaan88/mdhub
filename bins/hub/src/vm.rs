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
    None,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Hit rectangles, front-most last; the shell tests them in reverse.
    pub hits: Vec<(Rect, Hit)>,
    /// Content height of the scrolling list, so the shell can scroll it.
    pub list_content_h: f32,
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
