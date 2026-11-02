//! Pebbly's own timer and stopwatch. Pure: every call takes `now` (GetTickCount64 ms),
//! which keeps counting through sleep, so a timer that ends while the laptop sleeps is Done on wake.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Running,
    Paused,
    Done,
}

/// Counts down from `total_ms`. `banked_ms` is time already run before the current start.
#[derive(Clone, Debug)]
pub struct Timer {
    total_ms: u64,
    started_ms: Option<u64>,
    banked_ms: u64,
}

impl Timer {
    pub fn start(total_ms: u64, now: u64) -> Self {
        Timer { total_ms, started_ms: Some(now), banked_ms: 0 }
    }

    fn run_ms(&self, now: u64) -> u64 {
        self.banked_ms + self.started_ms.map_or(0, |s| now.saturating_sub(s))
    }

    pub fn remaining_ms(&self, now: u64) -> u64 {
        self.total_ms.saturating_sub(self.run_ms(now))
    }

    pub fn state(&self, now: u64) -> RunState {
        if self.remaining_ms(now) == 0 {
            RunState::Done
        } else if self.started_ms.is_some() {
            RunState::Running
        } else {
            RunState::Paused
        }
    }

    pub fn pause(&mut self, now: u64) {
        if self.started_ms.is_some() {
            self.banked_ms = self.run_ms(now);
            self.started_ms = None;
        }
    }

    pub fn resume(&mut self, now: u64) {
        if self.started_ms.is_none() {
            self.started_ms = Some(now);
        }
    }

    /// Done: restart with one minute left. Otherwise: one more minute on the total.
    pub fn add_minute(&mut self, now: u64) {
        if self.state(now) == RunState::Done {
            *self = Timer::start(60_000, now);
        } else {
            self.total_ms += 60_000;
        }
    }

    pub fn total_ms(&self) -> u64 {
        self.total_ms
    }
}

#[derive(Clone, Debug)]
pub struct Stopwatch {
    started_ms: Option<u64>,
    banked_ms: u64,
    laps: Vec<u64>,
    lap_mark_ms: u64,
}

impl Stopwatch {
    pub fn start(now: u64) -> Self {
        Stopwatch { started_ms: Some(now), banked_ms: 0, laps: Vec::new(), lap_mark_ms: 0 }
    }

    pub fn elapsed_ms(&self, now: u64) -> u64 {
        self.banked_ms + self.started_ms.map_or(0, |s| now.saturating_sub(s))
    }

    pub fn state(&self) -> RunState {
        if self.started_ms.is_some() { RunState::Running } else { RunState::Paused }
    }

    pub fn pause(&mut self, now: u64) {
        if self.started_ms.is_some() {
            self.banked_ms = self.elapsed_ms(now);
            self.started_ms = None;
        }
    }

    pub fn resume(&mut self, now: u64) {
        if self.started_ms.is_none() {
            self.started_ms = Some(now);
        }
    }

    /// Records a lap and returns its length.
    pub fn lap(&mut self, now: u64) -> u64 {
        let elapsed = self.elapsed_ms(now);
        let lap = elapsed - self.lap_mark_ms;
        self.lap_mark_ms = elapsed;
        self.laps.push(lap);
        lap
    }

    pub fn laps(&self) -> &[u64] {
        &self.laps
    }
}

fn clock(total_s: u64) -> String {
    let (h, m, s) = (total_s / 3600, total_s / 60 % 60, total_s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Remaining time, rounded up to whole seconds (a fresh 10 min timer reads "10:00").
pub fn format_timer(ms: u64) -> String {
    clock(ms.div_ceil(1000))
}

/// Elapsed whole seconds for the pill, which only ticks once a second: "00:12".
pub fn format_elapsed(ms: u64) -> String {
    clock(ms / 1000)
}

/// Elapsed time with tenths and a Turkish decimal comma: "00:12,4".
pub fn format_stopwatch(ms: u64) -> String {
    format!("{}{}{}", clock(ms / 1000), crate::i18n::decimal_sep(), ms / 100 % 10)
}

/// Human length for labels: "10 dk", "1 sa 30 dk", "45 sn".
pub fn format_duration_label(ms: u64) -> String {
    let total_s = ms / 1000;
    let (h, m, s) = (total_s / 3600, total_s / 60 % 60, total_s % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(crate::i18n::trf("{} sa", &[&h]));
    }
    if m > 0 {
        parts.push(crate::i18n::trf("{} dk", &[&m]));
    }
    if s > 0 && h == 0 {
        parts.push(crate::i18n::trf("{} sn", &[&s]));
    }
    if parts.is_empty() { crate::i18n::trf("{} sn", &[&0]) } else { parts.join(" ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_ceil_display() {
        let t = Timer::start(600_000, 0);
        assert_eq!(format_timer(t.remaining_ms(400)), "10:00");
        assert_eq!(format_timer(t.remaining_ms(1_000)), "09:59");
        assert_eq!(format_timer(3_909_000), "1:05:09");
    }

    #[test]
    fn timer_pause_resume() {
        let mut t = Timer::start(600_000, 0);
        t.pause(30_000);
        assert_eq!(t.state(60_000), RunState::Paused);
        t.resume(90_000);
        assert_eq!(t.remaining_ms(100_000), 560_000);
    }

    #[test]
    fn timer_done_at_zero() {
        let t = Timer::start(600_000, 0);
        assert_eq!(t.state(600_000), RunState::Done);
        assert_eq!(t.remaining_ms(600_000), 0);
    }

    #[test]
    fn timer_add_minute_after_done() {
        let mut t = Timer::start(600_000, 0);
        t.add_minute(700_000);
        assert_eq!(t.remaining_ms(700_000), 60_000);
        assert_eq!(t.state(700_000), RunState::Running);
    }

    #[test]
    fn timer_add_minute_while_running() {
        let mut t = Timer::start(600_000, 0);
        t.add_minute(100_000);
        assert_eq!(t.remaining_ms(100_000), 560_000);
        assert_eq!(t.total_ms(), 660_000);
    }

    #[test]
    fn timer_counts_time_across_ticks_gap() {
        let t = Timer::start(600_000, 1_000);
        assert_eq!(t.state(3_601_000), RunState::Done);
        assert_eq!(t.remaining_ms(3_601_000), 0);
    }

    #[test]
    fn stopwatch_laps() {
        let mut s = Stopwatch::start(0);
        assert_eq!(s.lap(12_400), 12_400);
        assert_eq!(s.lap(20_000), 7_600);
        assert_eq!(s.laps(), &[12_400, 7_600]);
        assert_eq!(format_stopwatch(12_449), "00:12,4");
        assert_eq!(format_stopwatch(3_612_400), "1:00:12,4");
    }

    #[test]
    fn stopwatch_pause_resume() {
        let mut s = Stopwatch::start(0);
        s.pause(5_000);
        assert_eq!(s.state(), RunState::Paused);
        s.resume(65_000);
        assert_eq!(s.elapsed_ms(70_000), 10_000);
    }

    #[test]
    fn elapsed_for_the_pill_has_no_tenths() {
        assert_eq!(format_elapsed(12_999), "00:12");
        assert_eq!(format_elapsed(3_725_000), "1:02:05");
    }

    #[test]
    fn labels() {
        assert_eq!(format_duration_label(600_000), "10 dk");
        assert_eq!(format_duration_label(5_400_000), "1 sa 30 dk");
        assert_eq!(format_duration_label(45_000), "45 sn");
    }
}
