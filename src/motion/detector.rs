use std::collections::VecDeque;

/// Compute mean absolute difference between two grayscale frames, normalized to [0, 1].
pub fn frame_intensity(prev: &[u8], curr: &[u8]) -> f64 {
    assert_eq!(prev.len(), curr.len());
    if prev.is_empty() {
        return 0.0;
    }
    let sum: u64 = prev
        .iter()
        .zip(curr.iter())
        .map(|(&a, &b)| (a as i32 - b as i32).unsigned_abs() as u64)
        .sum();
    sum as f64 / (prev.len() as f64 * 255.0)
}

/// Detects motion above a calibrated baseline.
pub struct MotionDetector {
    baseline: f64,
    threshold_multiplier: f64,
    /// Rolling window of recent intensities for smoothing.
    window: VecDeque<f64>,
    window_max: usize,
    /// Debounce: number of consecutive elevated readings required.
    debounce_count: usize,
    elevated_streak: usize,
}

impl MotionDetector {
    /// Create a new detector.
    ///
    /// - `baseline`: the calibrated resting intensity (from SnooCalibrator).
    /// - `threshold_multiplier`: how many times above baseline counts as motion.
    /// - `fps_estimate`: approximate FPS for smoothing window (0.5s worth of frames).
    /// - `debounce_secs`: how long intensity must stay elevated (default 0.3s).
    pub fn new(baseline: f64, threshold_multiplier: f64, fps_estimate: f64, debounce_secs: f64) -> Self {
        let window_max = (fps_estimate * 0.5).max(1.0) as usize;
        let debounce_count = (fps_estimate * debounce_secs).max(1.0) as usize;
        Self {
            baseline,
            threshold_multiplier,
            window: VecDeque::with_capacity(window_max),
            window_max,
            debounce_count,
            elevated_streak: 0,
        }
    }

    /// Feed a new intensity value. Returns Some(rolling_avg) if motion is detected.
    pub fn update(&mut self, intensity: f64) -> Option<f64> {
        self.window.push_back(intensity);
        if self.window.len() > self.window_max {
            self.window.pop_front();
        }

        let rolling_avg = self.window.iter().sum::<f64>() / self.window.len() as f64;
        let threshold = self.baseline * self.threshold_multiplier;

        if rolling_avg > threshold {
            self.elevated_streak += 1;
            if self.elevated_streak >= self.debounce_count {
                return Some(rolling_avg);
            }
        } else {
            self.elevated_streak = 0;
        }

        None
    }

    pub fn baseline(&self) -> f64 {
        self.baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_frames_zero_intensity() {
        let frame = vec![128u8; 100];
        assert_eq!(frame_intensity(&frame, &frame), 0.0);
    }

    #[test]
    fn opposite_frames_max_intensity() {
        let black = vec![0u8; 100];
        let white = vec![255u8; 100];
        let intensity = frame_intensity(&black, &white);
        assert!((intensity - 1.0).abs() < 1e-10);
    }

    #[test]
    fn detector_no_motion_below_threshold() {
        let mut det = MotionDetector::new(0.1, 3.0, 10.0, 0.3);
        // Feed values below threshold (0.1 * 3.0 = 0.3)
        for _ in 0..20 {
            assert!(det.update(0.05).is_none());
        }
    }

    #[test]
    fn detector_motion_above_threshold_after_debounce() {
        let mut det = MotionDetector::new(0.1, 3.0, 10.0, 0.3);
        // debounce_count = ceil(10 * 0.3) = 3
        // Feed high intensity values
        let mut detected = false;
        for _ in 0..10 {
            if det.update(0.5).is_some() {
                detected = true;
                break;
            }
        }
        assert!(detected);
    }
}
