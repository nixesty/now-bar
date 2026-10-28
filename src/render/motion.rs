//! Motion curves. The pill⇄card morph uses an under-damped spring (One UI 9's slight bulge),
//! sampled into key frames: the system's natural-motion spring only reports completion about a
//! second after it visibly settles, which left the pill hidden too long after closing.

/// Damping ratio of the morph spring (≈4 % overshoot).
pub const SPRING_DAMPING: f32 = 0.72;
/// Natural frequency in rad/s: peak at ~0.25 s, settled (2 %) at ~0.31 s, matching the
/// spike's system spring, approved as smooth.
pub const SPRING_OMEGA: f32 = 18.1;
pub const SPRING_DURATION_S: f32 = 0.45;

/// Animation durations are multiplied by this (settings: off, slow, normal, fast).
pub fn speed_factor() -> f32 {
    match crate::settings::current().animation_speed {
        crate::settings::AnimationSpeed::Off => 0.01,
        crate::settings::AnimationSpeed::Slow => 1.33,
        crate::settings::AnimationSpeed::Normal => 1.0,
        crate::settings::AnimationSpeed::Fast => 0.75,
    }
}

/// A duration in 100-ns ticks for `ms`, scaled by the speed setting (at least 1 ms).
pub fn scaled_ticks(ms: f32) -> i64 {
    ((ms * speed_factor()).max(1.0) * 10_000.0) as i64
}

/// Step response of the spring: 0 at t = 0, settling at 1.
pub fn spring_step(t_s: f32) -> f32 {
    let z = SPRING_DAMPING;
    let root = (1.0 - z * z).sqrt();
    let wd = SPRING_OMEGA * root;
    1.0 - (-z * SPRING_OMEGA * t_s).exp() * ((wd * t_s).cos() + z / root * (wd * t_s).sin())
}

/// `n` (progress, value) pairs over [`SPRING_DURATION_S`]; progress is in (0, 1] and the last
/// pair is exactly (1, 1) so the animation ends on its target.
pub fn spring_samples(n: usize) -> Vec<(f32, f32)> {
    (1..=n)
        .map(|i| {
            let p = i as f32 / n as f32;
            if i == n { (1.0, 1.0) } else { (p, spring_step(p * SPRING_DURATION_S)) }
        })
        .collect()
}

/// The spring `progress` of the way through its (scaled) duration: the curve, then exactly 1,
/// where its last key frame leaves it.
pub fn spring_at(progress: f32) -> f32 {
    if progress >= 1.0 { 1.0 } else { spring_step(progress.max(0.0) * SPRING_DURATION_S) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_starts_at_zero_and_settles_at_one() {
        assert_eq!(spring_step(0.0), 0.0);
        assert!((spring_step(SPRING_DURATION_S) - 1.0).abs() < 0.005);
    }

    #[test]
    fn spring_overshoots_slightly() {
        let peak = (0..=450).map(|ms| spring_step(ms as f32 / 1000.0)).fold(0.0f32, f32::max);
        assert!(peak > 1.03 && peak < 1.05, "peak {peak}");
    }

    #[test]
    fn spring_rises_monotonically_to_its_peak() {
        let v: Vec<f32> = (0..=250).map(|ms| spring_step(ms as f32 / 1000.0)).collect();
        assert!(v.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn samples_cover_the_duration_and_end_exactly() {
        let s = spring_samples(30);
        assert_eq!(s.len(), 30);
        assert_eq!(s.last().copied(), Some((1.0, 1.0)));
        assert!(s.windows(2).all(|w| w[1].0 > w[0].0));
    }

    #[test]
    fn spring_at_is_the_curve_until_the_end_then_one() {
        assert_eq!(spring_at(-0.2), 0.0);
        assert_eq!(spring_at(0.0), 0.0);
        assert_eq!(spring_at(0.5), spring_step(0.5 * SPRING_DURATION_S));
        // Past its peak but not done: what the card looks like while it still says it's opening.
        assert!(spring_at(0.7) > 1.0);
        assert_eq!(spring_at(1.0), 1.0);
        assert_eq!(spring_at(4.0), 1.0);
    }
}
