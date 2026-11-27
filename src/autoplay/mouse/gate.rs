//! Whether Majsoul autoplay may act.

use crate::config::MouseProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// No calibration has been finished yet.
    NotCalibrated,
    /// A calibration is recording; the user is the one playing.
    Calibrating,
}

pub fn majsoul_gate(profile: &MouseProfile, calibrating: bool) -> Result<(), Blocked> {
    if calibrating {
        Err(Blocked::Calibrating)
    } else if !profile.calibrated {
        Err(Blocked::NotCalibrated)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_calibrated_profile_outside_a_session_passes() {
        let mut p = MouseProfile::default();
        assert_eq!(majsoul_gate(&p, false), Err(Blocked::NotCalibrated));
        p.calibrated = true;
        assert_eq!(majsoul_gate(&p, false), Ok(()));
        assert_eq!(majsoul_gate(&p, true), Err(Blocked::Calibrating));
    }
}
