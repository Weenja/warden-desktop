//! Tray flyout maths for Windows, in physical pixels. No Win32 here, so the tests run on any OS.
//!
//! The taskbar can sit on any screen edge. The flyout opens from that edge, next to the tray
//! icon, like Windows' own flyouts (volume, network).

/// A screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Area {
    fn right(&self) -> f64 {
        self.x + self.width
    }
    fn bottom(&self) -> f64 {
        self.y + self.height
    }
    fn centre(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The taskbar's edge: the work-area edge nearest the tray icon. A docked taskbar puts the icon
/// outside the work area (negative distance); an auto-hiding one puts it just inside.
pub fn taskbar_edge(icon: Area, work: Area) -> Edge {
    let (cx, cy) = icon.centre();
    let distances = [
        (Edge::Bottom, work.bottom() - cy),
        (Edge::Top, cy - work.y),
        (Edge::Right, work.right() - cx),
        (Edge::Left, cx - work.x),
    ];
    distances.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(edge, _)| edge).unwrap_or(Edge::Bottom)
}

/// Top-left corner for a flyout of `size` (width, height): against the taskbar edge, `gap`
/// away from it, centred on the icon along that edge and kept inside the work area.
pub fn flyout_origin(icon: Area, size: (f64, f64), work: Area, gap: f64) -> (f64, f64) {
    let (width, height) = size;
    let (cx, cy) = icon.centre();
    let along_x = clamp(cx - width / 2.0, work.x + gap, work.right() - width - gap);
    let along_y = clamp(cy - height / 2.0, work.y + gap, work.bottom() - height - gap);
    match taskbar_edge(icon, work) {
        Edge::Bottom => (along_x, work.bottom() - height - gap),
        Edge::Top => (along_x, work.y + gap),
        Edge::Left => (work.x + gap, along_y),
        Edge::Right => (work.right() - width - gap, along_y),
    }
}

/// Like f64::clamp, but a window larger than the room left pins to the start instead of panicking.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.min(max).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: f64 = 12.0;
    const FLYOUT: (f64, f64) = (330.0, 520.0);

    fn area(x: f64, y: f64, width: f64, height: f64) -> Area {
        Area { x, y, width, height }
    }

    #[test]
    fn bottom_taskbar_opens_above_the_icon() {
        let work = area(0.0, 0.0, 1920.0, 1032.0);
        let icon = area(1700.0, 1036.0, 32.0, 40.0);
        assert_eq!(taskbar_edge(icon, work), Edge::Bottom);
        assert_eq!(flyout_origin(icon, FLYOUT, work, GAP), (1716.0 - 165.0, 1032.0 - 520.0 - GAP));
    }

    #[test]
    fn flyout_stays_inside_the_right_edge() {
        let work = area(0.0, 0.0, 1920.0, 1032.0);
        let icon = area(1880.0, 1036.0, 32.0, 40.0);
        assert_eq!(flyout_origin(icon, FLYOUT, work, GAP).0, 1920.0 - 330.0 - GAP);
    }

    #[test]
    fn top_taskbar_opens_below_the_icon() {
        let work = area(0.0, 48.0, 1920.0, 1032.0);
        let icon = area(1700.0, 4.0, 32.0, 40.0);
        assert_eq!(taskbar_edge(icon, work), Edge::Top);
        assert_eq!(flyout_origin(icon, FLYOUT, work, GAP).1, 48.0 + GAP);
    }

    #[test]
    fn side_taskbars_open_beside_the_icon() {
        let left_work = area(62.0, 0.0, 1858.0, 1080.0);
        let left_icon = area(15.0, 900.0, 32.0, 32.0);
        assert_eq!(taskbar_edge(left_icon, left_work), Edge::Left);
        assert_eq!(flyout_origin(left_icon, FLYOUT, left_work, GAP), (62.0 + GAP, 1080.0 - 520.0 - GAP));

        let right_work = area(0.0, 0.0, 1858.0, 1080.0);
        let right_icon = area(1873.0, 100.0, 32.0, 32.0);
        assert_eq!(taskbar_edge(right_icon, right_work), Edge::Right);
        assert_eq!(flyout_origin(right_icon, FLYOUT, right_work, GAP), (1858.0 - 330.0 - GAP, GAP));
    }

    #[test]
    fn auto_hiding_taskbar_uses_the_nearest_edge() {
        // Auto-hide: the work area is the whole screen and the icon sits inside it.
        let work = area(0.0, 0.0, 1920.0, 1080.0);
        let icon = area(1700.0, 1044.0, 32.0, 32.0);
        assert_eq!(taskbar_edge(icon, work), Edge::Bottom);
    }

    #[test]
    fn second_monitor_offsets_are_kept() {
        let work = area(-1920.0, 0.0, 1920.0, 1032.0);
        let icon = area(-300.0, 1036.0, 32.0, 40.0);
        let (x, y) = flyout_origin(icon, FLYOUT, work, GAP);
        assert_eq!((x, y), (-284.0 - 165.0, 1032.0 - 520.0 - GAP));
    }
}
