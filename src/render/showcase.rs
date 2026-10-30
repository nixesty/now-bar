//! The pills the setup's stage turns through: what the Now Bar shows, drawn by its own painter.

use windows::core::Result;

use super::color::team_palette;
use super::layout::Scale;
use super::offscreen::render;
use super::pill::{paint_pill_content, MatchPill, PillContent, PillTeam, PillThumb};
use crate::activities::buds::{BudsSnapshot, Worn};
use crate::activities::live::LiveState;
use crate::activities::media::{Caps, MediaSnapshot};
use crate::activities::shelf::FileKind;

/// Music, Claude at work, a live match, the Buds, the shelf (names made up).
pub fn pills() -> Vec<PillContent> {
    let song = MediaSnapshot {
        id: 1,
        app_id: "Spotify.exe".into(),
        app_name: "Spotify".into(),
        title: "Gece Yarısı Treni".into(),
        artist: "Deniz Kaya".into(),
        playing: true,
        timeline: None,
        can: Caps { play_pause: true, prev: true, next: true, seek: true, shuffle: false, repeat: false },
        shuffle: None,
        repeat: None,
        art: None,
        art_size: (0, 0),
        accent: [0x1D, 0xB9, 0x54],
        changed_ms: 0,
    };
    let team = |code: &str, primary: [u8; 3], secondary: [u8; 3]| PillTeam { code: code.into(), palette: team_palette(primary, secondary), crest: None };
    let game = MatchPill {
        id: 1,
        home: team("FB", [0x0E, 0x1E, 0x5B], [0xF5, 0xD1, 0x0D]),
        away: team("GS", [0xA9, 0x0B, 0x2B], [0xFD, 0xB9, 0x12]),
        score: Some((2, 1)),
        centre: "67'".into(),
        live: true,
        stale: false,
        dim: (false, false),
        fresh: None,
        moment: None,
        followed: None,
        wide: false,
    };
    let file = |kind| PillThumb { thumb: None, kind };
    vec![
        PillContent::media(song),
        PillContent::Live { key: 1, title: "now-bar".into(), status: "4:12".into(), claude: true, state: LiveState::Run, action: format!("{}: app.rs", crate::i18n::tr("Düzenliyor")) },
        PillContent::Match(Box::new(game)),
        PillContent::Buds(BudsSnapshot { left: 80, right: 75, case: Some(60), noise: None, worn: Worn { left: true, right: true }, ..Default::default() }),
        PillContent::Shelf {
            label: "sunum-final.pdf".into(),
            count: 3,
            thumbs: vec![file(FileKind::Pdf), file(FileKind::Image), file(FileKind::Archive)],
            drop_hint: false,
            min_width_dip: 0.0,
        },
    ]
}

/// One pill for the stage: its content alone (the body is the stage's, so it can glide), the
/// body's colours top to bottom (straight RGBA) and the accent sweeping in from the left.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    /// Premultiplied BGRA.
    pub px: Vec<u8>,
    pub top: [u8; 4],
    pub bottom: [u8; 4],
    pub accent: Option<(u8, u8, u8)>,
    /// The pill's colour (a song's cover, a team's, Claude's), for the light behind it.
    pub glow: (u8, u8, u8),
}

/// `content` drawn at `scale` on a transparent ground.
pub fn frame(content: &PillContent, scale: f32) -> Result<Frame> {
    let s = Scale(scale);
    let (w, h) = (s.even_px(content.width_dip()), s.px(32.0).round());
    let px = render(w as u32, h as u32, [0.0; 4], |ink| paint_pill_content(ink, content, w, h, &s))?;
    let (top, bottom) = content.base();
    let c = |c: windows::UI::Color| [c.R, c.G, c.B, c.A];
    Ok(Frame { w: w as u32, h: h as u32, px, top: c(top), bottom: c(bottom), accent: content.has_accent_gradient().then(|| content.accent()), glow: content.accent() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_takes_every_pill_at_full_size() {
        let widths: Vec<f32> = pills().iter().map(PillContent::width_dip).collect();
        assert!(widths.iter().all(|w| *w <= 300.0), "{widths:?}: the setup's stage shows 300 DIP at full size");
    }
}
