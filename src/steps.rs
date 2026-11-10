//! Rounding a value to the steps its field holds (vectors README, Encoding; interpretations.md
//! "Values between steps are rounded").

/// How close to a step's half, measured in steps, a value must be to count as the half. A neutral
/// value is decimal text whose nearest binary number often lies a few ulps either side of the half
/// the text means; this lets the half the text means win.
const HALF_TOLERANCE: f64 = 1e-6;

/// `steps` (a value measured in its field's steps) rounded to the nearest whole step, halves away
/// from zero, where a value within 1e-6 of a half is the half.
pub(crate) fn round(steps: f64) -> f64 {
    if !steps.is_finite() {
        return steps;
    }
    let magnitude = steps.abs();
    let floor = libm::floor(magnitude);
    let fraction = magnitude - floor;
    let rounded = if fraction > 0.5 || (fraction - 0.5).abs() <= HALF_TOLERANCE { floor + 1.0 } else { floor };
    // A value that rounds to 0 is 0, not -0.
    if steps < 0.0 && rounded != 0.0 { -rounded } else { rounded }
}

#[cfg(test)]
mod tests {
    use super::round;

    #[test]
    fn halves_go_away_from_zero() {
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), -1.0);
        assert_eq!(round(2.4), 2.0);
        assert_eq!(round(-0.3), 0.0);
        assert!(round(-0.3).is_sign_positive());
        assert_eq!(round(-0.6), -1.0);
    }

    #[test]
    fn a_value_within_the_tolerance_of_a_half_is_the_half() {
        // 58.584999999999996 minutes is the half 58.585.
        assert_eq!(round(0.9764166666666666 * 60.0 * 100.0), 5859.0);
        // 986.9499999999999 mbar is 9869.5 tenths.
        assert_eq!(round(986.9499999999999 * 10.0), 9870.0);
        assert_eq!(round(4.4999999997), 5.0);
        assert_eq!(round(4.4999), 4.0);
    }
}
