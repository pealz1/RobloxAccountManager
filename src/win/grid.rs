//! Tiles Roblox windows into a grid on the monitor under the cursor.
//!
//! Fixes upstream issue #79 / PR #82: uses the monitor **work area** (so windows
//! never hide behind the taskbar), accounts for each window's border, keeps a 16:9
//! client area, drops a column when Roblox refuses to shrink below its minimum
//! width, and only stacks rows vertically when a row cannot otherwise fit.

use crate::error::{AppError, AppResult};

const GAP: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    fn width(&self) -> i32 {
        self.right - self.left
    }
    fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// A window to place, with the border sizes measured from it.
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    pub hwnd: isize,
    pub frame_width: i32,
    pub frame_height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub hwnd: isize,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridResult {
    pub columns: usize,
    pub rows: usize,
}

/// Computes window placements inside `work_area`. `enforced_min_width` is the
/// smallest width Roblox will accept (0 when unknown); when a column would be
/// narrower than that, the layout drops a column and tries again.
pub fn plan(work_area: Rect, tiles: &[Tile], enforced_min_width: i32) -> AppResult<(Vec<Placement>, GridResult)> {
    if tiles.is_empty() {
        return Err(AppError::new("NO_ROBLOX_WINDOWS", "No Roblox Windows", "Open at least one Roblox window before tiling."));
    }
    let count = tiles.len();
    let available_width = work_area.width();
    let available_height = work_area.height();

    let mut columns = (count as f64).sqrt().ceil() as usize;
    loop {
        let rows = count.div_ceil(columns);
        let column_width = available_width / columns as i32;
        let cell_width = (column_width - GAP * 2).max(1);
        if enforced_min_width > 0 && cell_width < enforced_min_width && columns > 1 {
            columns -= 1;
            continue;
        }
        if columns == 1 && enforced_min_width > 0 && cell_width < enforced_min_width {
            return Err(AppError::new(
                "MONITOR_TOO_NARROW",
                "Monitor Too Narrow",
                "This monitor cannot fit a Roblox window at its minimum width.",
            ));
        }

        // Choose a window height: 16:9 of the client width, capped to the row height.
        let row_height = available_height / rows as i32;
        let mut placements = Vec::with_capacity(count);
        let mut max_height = 0;
        for (index, tile) in tiles.iter().enumerate() {
            let column = index % columns;
            let cell_left = work_area.left + column as i32 * available_width / columns as i32;
            let width = cell_width;
            let client_width = (width - tile.frame_width).max(1);
            let preferred = (client_width * 9 + 8) / 16 + tile.frame_height;
            let height = if count <= 2 { (row_height - GAP * 2).max(1) } else { preferred.min(row_height - GAP * 2).max(1) };
            max_height = max_height.max(height);
            placements.push((index, column, cell_left, width, height));
        }

        // Spread rows across the available height using the tallest window,
        // so rows never overlap and the bottom row stays inside the work area.
        let row_span = (available_height - max_height - GAP * 2).max(0);
        let mut out = Vec::with_capacity(count);
        for (index, column, cell_left, width, height) in placements {
            let row = index / columns;
            let x = cell_left + GAP;
            let y = if rows > 1 { work_area.top + GAP + row as i32 * row_span / (rows as i32 - 1) } else { work_area.top + GAP };
            out.push(Placement { hwnd: tiles[index].hwnd, rect: Rect { left: x, top: y, right: x + width, bottom: y + height } });
            let _ = column;
        }
        return Ok((out, GridResult { columns, rows }));
    }
}

/// Reads the work area of the monitor under the cursor, measures each Roblox
/// window's border, plans the grid and moves the windows into place.
#[cfg(windows)]
pub fn tile_roblox_windows() -> AppResult<GridResult> {
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetCursorPos, GetWindowRect, HWND_TOP, IsIconic, SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowPos, ShowWindow,
    };

    let windows: Vec<super::window::WindowInfo> = super::window::windows_for(&super::process::id_set().keys().copied().collect::<Vec<_>>())
        .into_iter()
        .filter(|w| w.area() > 0)
        .collect();
    if windows.is_empty() {
        return Err(AppError::new(
            "NO_ROBLOX_WINDOWS",
            "No Roblox Windows",
            "Open at least one visible Roblox window before using Window Grid.",
        ));
    }

    // Work area of the monitor under the cursor.
    let work = unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        Rect { left: info.rcWork.left, top: info.rcWork.top, right: info.rcWork.right, bottom: info.rcWork.bottom }
    };

    // Measure borders; restore minimised windows so their frame is real.
    let mut tiles = Vec::new();
    for window in &windows {
        let hwnd = window.hwnd as HWND;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let mut cr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut wr);
            GetClientRect(hwnd, &mut cr);
            tiles.push(Tile {
                hwnd: window.hwnd,
                frame_width: (wr.right - wr.left) - (cr.right - cr.left),
                frame_height: (wr.bottom - wr.top) - (cr.bottom - cr.top),
            });
        }
    }

    // First pass with no known minimum width; re-plan if Roblox enforces a wider window.
    let mut enforced_min = 0;
    for _ in 0..tiles.len().max(1) {
        let (placements, result) = plan(work, &tiles, enforced_min)?;
        let mut widest_rejected = 0;
        for placement in &placements {
            let hwnd = placement.hwnd as HWND;
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    placement.rect.width(),
                    placement.rect.height(),
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetWindowRect(hwnd, &mut wr);
                let actual_width = wr.right - wr.left;
                if actual_width > placement.rect.width() {
                    widest_rejected = widest_rejected.max(actual_width);
                }
            }
        }
        if widest_rejected == 0 {
            // Widths accepted: move each window, back to front, into position.
            for placement in placements.iter().rev() {
                unsafe {
                    SetWindowPos(
                        placement.hwnd as HWND,
                        HWND_TOP,
                        placement.rect.left,
                        placement.rect.top,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
            crate::log_info!("Window Grid arranged {} window(s) into {}x{}", placements.len(), result.columns, result.rows);
            return Ok(result);
        }
        enforced_min = widest_rejected;
    }
    Err(AppError::new("WINDOW_GRID_FAILED", "Window Grid Failed", "The Roblox windows could not be arranged."))
}

#[cfg(not(windows))]
pub fn tile_roblox_windows() -> AppResult<GridResult> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Window Grid is only available on Windows."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiles(n: usize) -> Vec<Tile> {
        (0..n).map(|i| Tile { hwnd: i as isize + 1, frame_width: 16, frame_height: 39 }).collect()
    }

    fn work() -> Rect {
        Rect { left: 0, top: 0, right: 3440, bottom: 1392 }
    }

    #[test]
    fn nine_windows_stay_inside_work_area() {
        let (places, grid) = plan(work(), &tiles(9), 0).unwrap();
        assert_eq!((grid.columns, grid.rows), (3, 3));
        for p in &places {
            assert!(p.rect.left >= work().left + GAP);
            assert!(p.rect.right <= work().right - GAP);
            assert!(p.rect.top >= work().top + GAP);
            assert!(p.rect.bottom <= work().bottom - GAP, "window below work area: {:?}", p.rect);
        }
    }

    #[test]
    fn no_horizontal_overlap() {
        let (places, grid) = plan(work(), &tiles(10), 0).unwrap();
        for a in 0..places.len() {
            for b in (a + 1)..places.len() {
                let (x, y) = (places[a].rect, places[b].rect);
                let separated = x.right <= y.left || y.right <= x.left || x.bottom <= y.top || y.bottom <= x.top;
                assert!(separated, "overlap between {:?} and {:?}", x, y);
            }
        }
        assert!(grid.columns >= 1);
    }

    #[test]
    fn minimum_width_drops_a_column() {
        // 9 windows on a 1920-wide monitor, each needing >=800px: 3 columns no longer fit.
        let (_, grid) = plan(Rect { left: 0, top: 0, right: 1920, bottom: 1040 }, &tiles(9), 800).unwrap();
        assert_eq!(grid.columns, 2);
    }

    #[test]
    fn single_column_too_narrow_errors() {
        let err = plan(Rect { left: 0, top: 0, right: 300, bottom: 1040 }, &tiles(4), 800).unwrap_err();
        assert_eq!(err.code, "MONITOR_TOO_NARROW");
    }

    #[test]
    fn keeps_sixteen_by_nine_client_area() {
        let (places, _) = plan(work(), &tiles(9), 0).unwrap();
        let p = places[0].rect;
        let client_w = (p.right - p.left - 16) as f64;
        let client_h = (p.bottom - p.top - 39) as f64;
        let row_height = (work().height() / 3 - GAP * 2 - 39) as f64;
        assert!((client_h - (client_w * 9.0 / 16.0).min(row_height)).abs() <= 1.0);
    }
}
