//! Window placement policy. Positions are in the virtual desktop's physical coordinates;
//! sizes are logical client pixels, converted at the destination monitor's DPI.
use hub_core::state::WindowPlacement;
use mdshared_ui::chrome::{TITLE_H, WIN_BTN_W};

pub const DEFAULT_SIZE: (u32, u32) = (860, 660);
pub const MIN_SIZE: (u32, u32) = (520, 360);

struct WorkArea {
    // i64 arithmetic also handles hostile saved coordinates at either i32 limit.
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    scale: f64,
    primary: bool,
}

impl WorkArea {
    fn physical(&self, logical: u32) -> i64 {
        (f64::from(logical) * self.scale).round() as i64
    }

    fn fit(&self, (w, h): (u32, u32)) -> (u32, u32) {
        let logical = |px: i64| (px as f64 / self.scale).floor() as u32;
        (
            w.min(logical(self.right - self.left)).max(MIN_SIZE.0),
            h.min(logical(self.bottom - self.top)).max(MIN_SIZE.1),
        )
    }

    fn centered(&self, (w, h): (u32, u32)) -> (i32, i32) {
        let x = self.left + (self.right - self.left - self.physical(w)).max(0) / 2;
        let y = self.top + (self.bottom - self.top - self.physical(h)).max(0) / 2;
        (x as i32, y as i32)
    }

    /// Is enough of the title bar reachable to get the window back? A sliver under the taskbar
    /// or the window's bottom edge on a monitor below does not count.
    fn title_width(&self, (x, y): (i32, i32), w: u32) -> Option<i64> {
        let (x, y) = (i64::from(x), i64::from(y));
        let title_h = self.physical(TITLE_H as u32);
        if self.bottom.min(y + title_h) - self.top.max(y) < self.physical(24) {
            return None;
        }
        let drag_w = w.saturating_sub((3.0 * WIN_BTN_W) as u32);
        let visible = self.right.min(x + self.physical(drag_w)) - self.left.max(x);
        (visible >= self.physical(64)).then_some(visible)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Initial {
    pub position: Option<(i32, i32)>,
    pub size: (u32, u32),
    pub maximized: bool,
}

fn initial(
    size_override: Option<(u32, u32)>,
    saved: Option<WindowPlacement>,
    areas: &[WorkArea],
) -> Initial {
    let primary = areas.iter().find(|m| m.primary).or_else(|| areas.first());
    if let Some((w, h)) = size_override {
        let size = (w.max(MIN_SIZE.0), h.max(MIN_SIZE.1));
        return Initial {
            position: primary.map(|m| m.centered(size)),
            size,
            maximized: false,
        };
    }
    if let Some(saved) = saved
        && let Some(area) = areas
            .iter()
            .filter_map(|m| {
                m.title_width(saved.position, m.fit(saved.size).0)
                    .map(|visible| (m, visible))
            })
            .max_by_key(|&(m, width)| {
                // Prefer the monitor of the saved origin if the title is reachable there.
                let x = i64::from(saved.position.0);
                (m.left <= x && x < m.right, width)
            })
            .map(|(m, _)| m)
    {
        return Initial {
            position: Some(saved.position),
            size: area.fit(saved.size),
            maximized: saved.maximized,
        };
    }
    let size = primary.map_or(DEFAULT_SIZE, |m| m.fit(DEFAULT_SIZE));
    Initial {
        position: primary.map(|m| m.centered(size)),
        size,
        maximized: false,
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsIconic, IsZoomed, MONITORINFOF_PRIMARY,
    };
    use winit::event_loop::ActiveEventLoop;
    use winit::platform::windows::MonitorHandleExtWindows;
    use winit::window::Window;

    pub fn for_open(
        event_loop: &ActiveEventLoop,
        size_override: Option<(u32, u32)>,
        saved: Option<WindowPlacement>,
    ) -> Initial {
        let areas: Vec<_> = event_loop
            .available_monitors()
            .filter_map(|m| {
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                // SAFETY: the handle belongs to the monitor, and info is writable and sized.
                if unsafe { GetMonitorInfoW(m.hmonitor() as _, &mut info) } == 0 {
                    return None;
                }
                let r = info.rcWork;
                let scale = m.scale_factor();
                (r.right > r.left && r.bottom > r.top && scale.is_finite() && scale > 0.0)
                    .then_some(WorkArea {
                        left: r.left.into(),
                        top: r.top.into(),
                        right: r.right.into(),
                        bottom: r.bottom.into(),
                        scale,
                        primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
                    })
            })
            .collect();
        initial(size_override, saved, &areas)
    }

    /// Do not take an enlarged or iconic rectangle for the normal size. Query Windows rather
    /// than winit's event-driven flag: Moved can arrive before the matching maximize Resized.
    pub fn observe(
        w: &Window,
        hwnd: isize,
        last: Option<WindowPlacement>,
    ) -> Option<WindowPlacement> {
        let hwnd = hwnd as HWND;
        // SAFETY: hwnd is the live handle of w and these queries do not retain it.
        if unsafe { IsIconic(hwnd) != 0 } {
            // Windows may report no maximize state once minimized. Keep the last visible state.
            return last;
        }
        // SAFETY: same live window handle.
        if unsafe { IsZoomed(hwnd) != 0 } {
            return last.map(|p| WindowPlacement {
                maximized: true,
                ..p
            });
        }
        let mut outer = RECT::default();
        let mut client = RECT::default();
        // SAFETY: both RECTs are writable and hwnd is the same live window.
        if unsafe { GetWindowRect(hwnd, &mut outer) == 0 || GetClientRect(hwnd, &mut client) == 0 }
        {
            return last;
        }
        let scale = w.scale_factor();
        let logical = |n: i32| (f64::from(n) / scale).round() as u32;
        let size = (logical(client.right), logical(client.bottom));
        (size.0 > 0 && size.1 > 0)
            .then_some(WindowPlacement {
                position: (outer.left, outer.top),
                size,
                maximized: false,
            })
            .or(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1600,
        bottom: 900,
        scale: 1.0,
        primary: true,
    };
    const LEFT: WorkArea = WorkArea {
        left: -1920,
        top: 10,
        right: 0,
        bottom: 1050,
        scale: 1.5,
        primary: false,
    };
    const SAVED: WindowPlacement = WindowPlacement {
        position: (-1800, 100),
        size: (800, 520),
        maximized: true,
    };

    #[test]
    fn a_missing_monitor_or_unreachable_title_falls_back_to_centered_default() {
        for position in [
            SAVED.position,
            (30, -100),
            (1580, 10),
            (-694, 100), // Only the window buttons remain on-screen.
            (i32::MAX, i32::MIN),
        ] {
            assert_eq!(
                initial(Some((860, 660)), None, &[PRIMARY]),
                initial(
                    None,
                    Some(WindowPlacement { position, ..SAVED }),
                    &[PRIMARY]
                ),
                "{position:?}"
            );
        }
        assert_eq!(
            initial(None, Some(SAVED), &[PRIMARY]),
            Initial {
                position: Some((370, 120)),
                size: (860, 660),
                maximized: false
            }
        );
    }

    #[test]
    fn a_saved_size_below_the_minimum_is_clamped() {
        let saved = WindowPlacement {
            size: (100, 30),
            ..SAVED
        };
        assert_eq!(
            initial(None, Some(saved), &[LEFT, PRIMARY]),
            Initial {
                position: Some(saved.position),
                size: MIN_SIZE,
                maximized: true
            }
        );
    }

    #[test]
    fn a_negative_origin_and_logical_size_survive_a_different_dpi() {
        // The leftmost 100 physical pixels of the window are outside the monitor, but its
        // title is reachable and the 800x520 logical client fits at 150%.
        let saved = WindowPlacement {
            position: (-2020, 100),
            ..SAVED
        };
        assert_eq!(
            initial(None, Some(saved), &[LEFT, PRIMARY]),
            Initial {
                position: Some(saved.position),
                size: saved.size,
                maximized: true
            }
        );
        // This title is reachable only when visibility uses the 150%-scaled width.
        let saved = WindowPlacement {
            position: (-2760, 100),
            ..SAVED
        };
        assert_eq!(
            initial(None, Some(saved), &[LEFT, PRIMARY]).position,
            Some(saved.position)
        );
    }

    #[test]
    fn a_hostile_huge_saved_size_is_bounded_by_the_destination_work_area() {
        let saved = WindowPlacement {
            size: (u32::MAX, u32::MAX),
            ..SAVED
        };
        assert_eq!(
            initial(None, Some(saved), &[LEFT, PRIMARY]).size,
            (1280, 693)
        );
    }

    #[test]
    fn cli_size_overrides_all_saved_placement() {
        let args = crate::cli::parse(&["--size".into(), "740x450".into()]).unwrap();
        assert_eq!(
            initial(args.size, Some(SAVED), &[LEFT, PRIMARY]),
            Initial {
                position: Some((430, 225)),
                size: (740, 450),
                maximized: false
            }
        );
    }

    #[test]
    fn default_height_fits_the_seven_app_list_and_its_details_above_the_footer() {
        let vm = crate::demo::build("claude-switch", crate::vm::View::List).unwrap();
        let (w, h) = DEFAULT_SIZE;
        let g = crate::view::geometry(w as f32, h as f32, &vm);
        assert!(g.list.h >= g.content_h);
        assert!(g.pane.h >= crate::view::PANE_MIN);
        assert_eq!(g.pane.bottom(), g.status_y);
    }
}
