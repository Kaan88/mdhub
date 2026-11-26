//! What the window shows and what a pointer can hit. The drawing code (`view.rs`, `grid.rs`,
//! `ui.rs`) reads a `Vm` and returns a `Layout`; the shell (`app.rs`) builds the `Vm` from
//! hub-core and turns a `Hit` into work. Neither side owns the other's files.
use hub_core::model::Row;
use mdshared_ui::gfx::Rect;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferKey {
    /// None means Hub's own restart offer.
    pub app: Option<&'static str>,
    pub version: String,
    /// A freshly issued offer is distinct even when its app and version are unchanged.
    pub serial: u64,
}

#[derive(Clone, Debug)]
pub struct Banner {
    pub key: OfferKey,
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
    /// How many apps are eligible for Update all, independent of the filter.
    pub updates: usize,
    /// Filter text of the apps filter box.
    pub filter: String,
    pub filter_focus: bool,
    /// The caption draws Restore instead of Maximize. Offscreen defaults to false.
    pub maximized: bool,
    /// The notes control reached with Tab, if any.
    pub notes_focus: Hit,
    /// A picture of the settings card over the window (`--demo settings-...`): its rows and the
    /// category shown. The real window keeps a form of its own, so this is None there.
    pub settings: Option<(Vec<mdshared_ui::settings::Row>, usize)>,
    /// Settled overlay specimens; never supplied by a live engine.
    pub demo_menu: bool,
    pub demo_dropdown: bool,
    /// The release notes view: when it is Some, it takes the content area (the list and the pane
    /// are not drawn) until it is closed.
    pub notes: Option<NotesVm>,
    /// What the pointer is over and what it holds down: the buttons draw their hover and press
    /// from it. The shell keeps it; a demo sets it to show a hovered button.
    pub pointer: Pointer,
}

/// The control under the pointer and the one the left button went down on (both `Hit::None`
/// when there is none). A held control looks pressed only while the pointer is still over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub hot: Hit,
    pub held: Hit,
}

impl Pointer {
    /// Is the pointer over `h`?
    pub fn over(&self, h: Hit) -> bool {
        h != Hit::None && self.hot == h
    }

    /// Is `h` held down with the pointer still over it?
    pub fn down(&self, h: Hit) -> bool {
        self.over(h) && self.held == h
    }

    /// How the control `h` is to be drawn.
    pub fn look(&self, h: Hit) -> Look {
        Look {
            hovered: self.over(h),
            pressed: self.down(h),
        }
    }
}

/// A button's hover and press, as the drawing code takes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub hovered: bool,
    pub pressed: bool,
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
    /// The scrollbar is drawn active (wide): the pointer is on it or drags its thumb. The shell's
    /// state, set before each frame; the engine leaves it false.
    pub bar_active: bool,
}

/// What a row or tile's button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Act {
    Open,
    Install,
    Update,
    Adopt,
    Cancel,
    TryAgain,
}

/// What a point on the window is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
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
    /// The compact search button opens the filter field.
    FilterOpen,
    Filter,
    FilterClear,
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
    /// The offer bar's X: it means what its No button means, but it lights on its own.
    BannerClose,
    /// The pane's "Release notes" link of row `i`.
    Notes(usize),
    /// The notes view's back arrow.
    NotesBack,
    /// The notes view's "Try again".
    NotesRetry,
    /// The notes view's "GitHub": the release page in the browser.
    NotesPage,
    /// The notes view's scrollbar: its track (a click pages toward it) and its thumb (dragged).
    NotesTrack,
    NotesThumb,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppControl {
    Row,
    Action(Act),
    Menu,
    Notes,
}

/// A click belongs to what was pressed, not to the row or button slot that drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Control(Hit),
    App(&'static str, View, AppControl),
    Offer(OfferKey, Hit),
    Notes(&'static str, String, Hit),
}

impl Target {
    pub fn at(hit: Hit, vm: &Vm) -> Option<Self> {
        let app = |i: usize, control| {
            let app = vm.apps.get(i)?;
            (vm.notes.is_none()).then_some(Self::App(app.id, vm.view, control))
        };
        let command_bar = || vm.notes.is_none();
        match hit {
            Hit::Drag | Hit::NotesTrack | Hit::NotesThumb => None,
            Hit::Row(i) => app(i, AppControl::Row),
            Hit::Action(i, act)
                if crate::view::row_actions(vm.apps.get(i)?)
                    .iter()
                    .any(|b| b.act == act) =>
            {
                app(i, AppControl::Action(act))
            }
            Hit::Menu(i) => app(i, AppControl::Menu),
            Hit::Notes(i) if i == vm.sel && vm.apps.get(i)?.latest.is_some() => {
                app(i, AppControl::Notes)
            }
            Hit::Action(..) | Hit::Notes(_) => None,
            Hit::BannerYes | Hit::BannerNo | Hit::BannerClose => {
                Some(Self::Offer(vm.banner.as_ref()?.key.clone(), hit))
            }
            Hit::NotesBack | Hit::NotesPage | Hit::NotesRetry => {
                let notes = vm.notes.as_ref()?;
                (hit != Hit::NotesRetry || matches!(notes.state, NotesState::Failed(_)))
                    .then(|| Self::Notes(notes.id, notes.version.clone(), hit))
            }
            Hit::UpdateAll if !command_bar() || vm.updates == 0 || vm.busy.is_some() => None,
            Hit::CheckNow
                if !command_bar() || vm.checked.starts_with("Checking") || vm.busy.is_some() =>
            {
                None
            }
            Hit::CancelAll if !command_bar() || vm.busy.is_none() => None,
            Hit::FilterClear if !command_bar() || vm.filter.is_empty() => None,
            Hit::FilterOpen if vm.filter_focus => None,
            Hit::Filter | Hit::FilterOpen | Hit::Settings | Hit::ViewSwitch(_)
                if !command_bar() =>
            {
                None
            }
            Hit::None if !command_bar() || !vm.filter_focus => None,
            _ => Some(Self::Control(hit)),
        }
    }

    pub fn live_hit(&self, vm: &Vm) -> Option<Hit> {
        let hit = match self {
            Self::Control(hit) | Self::Offer(_, hit) | Self::Notes(_, _, hit) => *hit,
            Self::App(id, _, control) => {
                let i = vm.apps.iter().position(|a| a.id == *id)?;
                match control {
                    AppControl::Row => Hit::Row(i),
                    AppControl::Action(act) => Hit::Action(i, *act),
                    AppControl::Menu => Hit::Menu(i),
                    AppControl::Notes => Hit::Notes(i),
                }
            }
        };
        (Self::at(hit, vm).as_ref() == Some(self)).then_some(hit)
    }

    /// Return the current hit only if the pointer is on the original target in the last frame
    /// and that target still exists in the live model (a worker event can precede the next draw).
    pub fn release(&self, hit: Hit, painted: &Vm, live: &Vm) -> Option<Hit> {
        (self.live_hit(painted) == Some(hit))
            .then(|| self.live_hit(live))
            .flatten()
    }
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
    /// The notes view's scrollbar as drawn, (track, thumb), when the text can scroll.
    pub notes_bar: Option<(Rect, Rect)>,
    /// The chrome as it was drawn: the title row, the command bar (not in the notes view) and the
    /// status bar. What a window backdrop shows through (`frost.rs`); the list, the pane, the
    /// offer bar and the notes page are not in it.
    pub chrome: Vec<Rect>,
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
