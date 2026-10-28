//! Physical-pixel layout: DIP scaling and where the pill and cards sit.

/// DIP to physical-pixel factor (`dpi / 96`). One One UI dp is one DIP.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale(pub f32);

impl Scale {
    pub fn from_dpi(dpi: u32) -> Self {
        Scale(dpi as f32 / 96.0)
    }

    /// Nearest whole pixel.
    pub fn px(&self, dip: f32) -> f32 {
        (dip * self.0).round()
    }

    /// Nearest even pixel count, so centring never lands on a half pixel.
    pub fn even_px(&self, dip: f32) -> f32 {
        (dip * self.0 / 2.0).round() * 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RectPx {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarAlign {
    Left,
    Center,
}

#[derive(Clone, Copy, Debug)]
pub struct TaskbarInfo {
    pub rect: RectPx,
    pub align: TaskbarAlign,
    pub widgets: bool,
    /// At the bottom of the screen (a top or side taskbar comes from StartAllBack,
    /// ExplorerPatcher or Windhawk: the pill and cards assume the bottom).
    pub bottom: bool,
}

/// Where the pill sits, in screen pixels. With left-aligned icons the centre of the taskbar
/// is empty, so the pill is centred; with centred icons it goes to the left (after Widgets).
/// `offset_dip` (a setting) moves it right (+) or left (−).
pub fn pill_rect(bar: &TaskbarInfo, w_dip: f32, h_dip: f32, s: &Scale, offset_dip: f32) -> RectPx {
    let w = s.even_px(w_dip) as i32;
    let h = s.px(h_dip) as i32;
    let x = match bar.align {
        TaskbarAlign::Left => bar.rect.x + (bar.rect.w - w) / 2,
        TaskbarAlign::Center => bar.rect.x + s.px(if bar.widgets { 170.0 } else { 12.0 }) as i32,
    } + s.px(offset_dip) as i32;
    let y = bar.rect.y + ((bar.rect.h - h) as f32 / 2.0).round() as i32;
    RectPx { x, y, w, h }
}

/// Where the pill goes once the taskbar's own items are known (`taken`: their x spans in screen
/// pixels: Start, the app buttons, the tray...): `home` while it is clear of them, else the
/// nearest spot on the bar that keeps `gap` px from every item, counting `tail` px of slivers
/// on its right. None when no free stretch is wide enough.
pub fn clear_of(home: RectPx, tail: i32, gap: i32, bar: &TaskbarInfo, taken: &[(i32, i32)]) -> Option<RectPx> {
    let need = home.w + tail;
    let mut spans: Vec<(i32, i32)> = taken.iter().filter(|(a, b)| b > a).map(|&(a, b)| (a - gap, b + gap)).collect();
    spans.sort_unstable();
    let (lo, hi) = (bar.rect.x, bar.rect.right());
    let mut best: Option<i32> = None;
    let mut start = lo;
    // Each free stretch runs from where the items so far end to where the next one starts.
    for (a, b) in spans.into_iter().chain([(hi, hi)]) {
        let end = a.min(hi);
        if end - start >= need {
            let x = home.x.clamp(start, end - need);
            if best.is_none_or(|bx| (x - home.x).abs() < (bx - home.x).abs()) {
                best = Some(x);
            }
        }
        start = start.max(b);
    }
    best.map(|x| RectPx { x, ..home })
}

/// The pill's spot: full size where it fits ([`clear_of`]), else as its circle (`circle`: the
/// compact pill's rect at home); true when compact. None when not even the circle fits.
pub fn fit_pill(home: RectPx, circle: RectPx, tail: i32, gap: i32, bar: &TaskbarInfo, taken: &[(i32, i32)]) -> Option<(RectPx, bool)> {
    clear_of(home, tail, gap, bar, taken)
        .map(|r| (r, false))
        .or_else(|| clear_of(circle, tail, gap, bar, taken).map(|r| (r, true)))
}

/// The pill size setting's `factor`, made smaller where the taskbar (`bar_h` px tall) is too low
/// for a pill `h_dip` tall at `s` to keep 3 DIP above and below it.
pub fn pill_fit(bar_h: i32, h_dip: f32, s: &Scale, factor: f32) -> f32 {
    let room = bar_h as f32 - 2.0 * s.px(3.0);
    factor.min(room / (h_dip * s.0))
}

/// A card centred on `center_x`, its bottom `gap_dip` above the taskbar.
pub fn card_rect(bar: &TaskbarInfo, center_x: i32, w_dip: f32, h_dip: f32, gap_dip: f32, s: &Scale) -> RectPx {
    let w = s.even_px(w_dip) as i32;
    let h = s.px(h_dip) as i32;
    let bottom = bar.rect.y - s.px(gap_dip) as i32;
    // Centred on the pill, but never off the taskbar's monitor (a pill at the far left: icons
    // centred with Widgets off, or a large offset).
    let margin = s.px(8.0) as i32;
    let x = (center_x - w / 2).min(bar.rect.x + bar.rect.w - margin - w).max(bar.rect.x + margin);
    RectPx { x, y: bottom - h, w, h }
}

/// The card window's left edge for a `w` px wide window centred on `center_x`, kept on the
/// taskbar's monitor (the card inside it is kept 8 DIP in, so it always fits).
pub fn frame_x(bar: &TaskbarInfo, center_x: i32, w: i32) -> i32 {
    (center_x - w / 2).min(bar.rect.x + bar.rect.w - w).max(bar.rect.x)
}

/// The rect `t` of the way from `a` to `b`, past `b` while a spring overshoots: where the
/// morph's expressions put the shape.
pub fn lerp_rect(a: RectPx, b: RectPx, t: f32) -> RectPx {
    let mix = |p: i32, q: i32| (p as f32 + (q - p) as f32 * t).round() as i32;
    RectPx { x: mix(a.x, b.x), y: mix(a.y, b.y), w: mix(a.w, b.w), h: mix(a.h, b.h) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(align: TaskbarAlign, widgets: bool) -> TaskbarInfo {
        TaskbarInfo { rect: RectPx { x: 0, y: 1030, w: 1920, h: 50 }, align, widgets, bottom: true }
    }

    #[test]
    fn a_rect_between_two_follows_the_spring_past_the_end() {
        let pill = RectPx { x: 860, y: 1036, w: 200, h: 40 };
        let card = RectPx { x: 760, y: 586, w: 400, h: 438 };
        assert_eq!(lerp_rect(pill, card, 0.0), pill);
        assert_eq!(lerp_rect(pill, card, 1.0), card);
        assert_eq!(lerp_rect(pill, card, 0.5), RectPx { x: 810, y: 811, w: 300, h: 239 });
        // The spring overshoots a little, and so does the shape.
        assert_eq!(lerp_rect(pill, card, 1.04), RectPx { x: 756, y: 568, w: 408, h: 454 });
    }

    #[test]
    fn a_card_stays_on_the_taskbars_monitor() {
        let s = Scale::from_dpi(120);
        let b = bar(TaskbarAlign::Center, false);
        let left = card_rect(&b, 15, 340.0, 77.0, 12.0, &s);
        assert_eq!(left.x, s.px(8.0) as i32, "a pill at the far left: the card stops 8 DIP in");
        let right = card_rect(&b, 1915, 340.0, 77.0, 12.0, &s);
        assert_eq!(right.x + right.w, 1920 - s.px(8.0) as i32);
        assert_eq!(card_rect(&b, 960, 340.0, 77.0, 12.0, &s).x, 960 - 426 / 2, "in the middle: centred on the pill");
    }

    #[test]
    fn the_card_window_stays_on_the_monitor_too() {
        let b = bar(TaskbarAlign::Center, false);
        assert_eq!(frame_x(&b, 15, 600), 0);
        assert_eq!(frame_x(&b, 1915, 600), 1920 - 600);
        assert_eq!(frame_x(&b, 960, 600), 660);
    }

    #[test]
    fn scale_rounding() {
        let s = Scale::from_dpi(120);
        assert_eq!(s.px(12.0), 15.0);
        assert_eq!(s.even_px(242.0), 302.0);
        assert_eq!(s.even_px(340.0), 426.0);
        assert_eq!(s.px(350.0), 438.0);
    }

    #[test]
    fn pill_centred_on_left_aligned_taskbar() {
        assert_eq!(
            pill_rect(&bar(TaskbarAlign::Left, false), 176.0, 32.0, &Scale::from_dpi(120), 0.0),
            RectPx { x: 850, y: 1035, w: 220, h: 40 }
        );
    }

    #[test]
    fn pill_left_when_icons_centred() {
        let s = Scale::from_dpi(120);
        assert_eq!(pill_rect(&bar(TaskbarAlign::Center, false), 176.0, 32.0, &s, 0.0).x, 15);
        assert_eq!(pill_rect(&bar(TaskbarAlign::Center, true), 176.0, 32.0, &s, 0.0).x, 213);
    }

    /// The pill offset setting moves the pill (and with it the cards) either way.
    #[test]
    fn pill_offset_moves_it() {
        let (b, s) = (bar(TaskbarAlign::Left, false), Scale::from_dpi(120));
        let centred = pill_rect(&b, 300.0, 32.0, &s, 0.0);
        assert_eq!(pill_rect(&b, 300.0, 32.0, &s, -20.0).x, centred.x - 25);
        assert_eq!(pill_rect(&b, 300.0, 32.0, &s, 40.0).x, centred.x + 50);
    }

    /// A taskbar at 125 %: Start, Search, then 55 px app buttons from 124 px; the tray from
    /// 1424 px. A media pill (300 DIP) is 376 px wide, its slivers 30 px, the gap 10 px.
    fn items_at_125(buttons: i32) -> Vec<(i32, i32)> {
        let mut v = vec![(0, 69), (69, 124)];
        v.extend((0..buttons).map(|i| (124 + 55 * i, 179 + 55 * i)));
        v.push((1424, 1920));
        v
    }

    fn media_home() -> RectPx {
        pill_rect(&bar(TaskbarAlign::Left, false), 300.0, 32.0, &Scale::from_dpi(120), 0.0)
    }

    #[test]
    fn pill_stays_home_while_the_buttons_end_before_it() {
        let home = media_home();
        assert_eq!(home.x, 772);
        // Ten buttons end at 674 px.
        assert_eq!(clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(10)), Some(home));
    }

    /// Nothing known (UI Automation failed): where it always was.
    #[test]
    fn pill_stays_home_without_items() {
        let home = media_home();
        assert_eq!(clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &[]), Some(home));
    }

    #[test]
    fn pill_moves_right_of_the_last_button() {
        let home = media_home();
        // Twelve buttons end at 784 px, inside the pill: it starts a gap after them.
        let moved = clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(12)).unwrap();
        assert_eq!(moved, RectPx { x: 794, ..home });
        // Fourteen: further.
        assert_eq!(clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(14)).unwrap().x, 904);
    }

    /// The pill and its slivers end a gap before the tray, wherever the setting wants it.
    #[test]
    fn pill_and_slivers_stop_before_the_tray() {
        let s = Scale::from_dpi(120);
        let home = pill_rect(&bar(TaskbarAlign::Left, false), 300.0, 32.0, &s, 300.0);
        assert_eq!(home.x, 1147);
        let placed = clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(10)).unwrap();
        assert_eq!(placed.x + placed.w + 30, 1414);
    }

    #[test]
    fn no_room_between_the_buttons_and_the_tray() {
        let home = media_home();
        // Seventeen buttons end at 1059 px: 345 px are free, the pill and slivers need 406.
        assert_eq!(clear_of(home, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(17)), None);
    }

    /// Centred icons that grow over the pill's place after Widgets: the pill takes the free
    /// stretch on the other side of them when only that one is wide enough.
    #[test]
    fn centred_icons_send_the_pill_across() {
        let s = Scale::from_dpi(120);
        let b = bar(TaskbarAlign::Center, true);
        let home = pill_rect(&b, 220.0, 32.0, &s, 0.0);
        let items = [(0, 190), (380, 1000), (1424, 1920)];
        assert_eq!(clear_of(home, 30, 10, &b, &items), Some(RectPx { x: 1010, ..home }));
        // Wider icons leave no room anywhere.
        assert_eq!(clear_of(home, 30, 10, &b, &[(0, 190), (380, 1180), (1424, 1920)]), None);
    }

    /// The nearest free spot wins, and items come in any order, overlapping or empty.
    #[test]
    fn nearest_free_spot_from_unsorted_items() {
        let b = bar(TaskbarAlign::Left, false);
        let home = RectPx { x: 600, y: 1035, w: 200, h: 40 };
        let items = [(1424, 1920), (500, 640), (0, 69), (300, 300), (69, 520)];
        // Left of 500 is taken from 0; right of 640 is free: 650.
        assert_eq!(clear_of(home, 0, 10, &b, &items).unwrap().x, 650);
        // A free stretch on the left that is nearer wins over the right.
        let items = [(0, 69), (300, 690), (1424, 1920)];
        assert_eq!(clear_of(RectPx { x: 250, ..home }, 0, 10, &b, &items).unwrap().x, 90);
    }

    /// The compact pill's circle at home: as tall as the pill, centred like it.
    fn circle_home() -> RectPx {
        let r = pill_rect(&bar(TaskbarAlign::Left, false), 32.0, 32.0, &Scale::from_dpi(120), 0.0);
        RectPx { w: r.h, ..r }
    }

    #[test]
    fn full_pill_while_it_fits() {
        let home = media_home();
        assert_eq!(fit_pill(home, circle_home(), 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(14)), Some((RectPx { x: 904, ..home }, false)));
    }

    /// Seventeen buttons leave 345 px; the media pill needs 406, its circle 70.
    #[test]
    fn circle_when_the_full_pill_has_no_room() {
        let circle = circle_home();
        assert_eq!(circle, RectPx { x: 940, y: 1035, w: 40, h: 40 });
        assert_eq!(fit_pill(media_home(), circle, 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(17)), Some((RectPx { x: 1069, ..circle }, true)));
    }

    #[test]
    fn nothing_when_not_even_the_circle_fits() {
        // Twenty-two buttons end at 1334 px: the circle and slivers (70 px) just fit between 1344
        // and the tray's gap at 1414; twenty-three don't.
        assert!(fit_pill(media_home(), circle_home(), 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(22)).is_some());
        assert_eq!(fit_pill(media_home(), circle_home(), 30, 10, &bar(TaskbarAlign::Left, false), &items_at_125(23)), None);
    }

    /// Windows 11's taskbar (48 DIP) has room for every size; Windows 10's (40, or 30 with small
    /// buttons) makes the pill smaller so it keeps 3 DIP above and below.
    #[test]
    fn pill_fits_the_taskbars_height() {
        let one = Scale::from_dpi(96);
        assert_eq!(pill_fit(48, 32.0, &one, 1.125), 1.125, "Windows 11, Large");
        assert_eq!(pill_fit(40, 32.0, &one, 1.0), 1.0, "Windows 10, Standard");
        assert_eq!(pill_fit(40, 32.0, &one, 1.125), 34.0 / 32.0, "Windows 10, Large: 34 px");
        assert_eq!(pill_fit(30, 32.0, &one, 0.875), 0.75, "small buttons: 24 px");
        let s = Scale::from_dpi(144);
        assert_eq!(pill_fit(72, 32.0, &s, 1.125), 1.125, "Windows 11 at 150 %");
        assert_eq!(pill_fit(45, 32.0, &s, 1.0), 35.0 / 48.0, "small buttons at 150 %: 35 px");
    }

    #[test]
    fn card_sits_12dip_above_taskbar() {
        assert_eq!(
            card_rect(&bar(TaskbarAlign::Left, false), 960, 340.0, 350.0, 12.0, &Scale::from_dpi(120)),
            RectPx { x: 747, y: 577, w: 426, h: 438 }
        );
    }
}
