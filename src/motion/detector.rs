use std::collections::VecDeque;

/// Precomputed grid geometry for dividing frames into cells.
pub struct GridConfig {
    pub cols: u32,
    pub rows: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    cell_width: u32,
    cell_height: u32,
    pub num_cells: usize,
}

impl GridConfig {
    pub fn new(frame_width: u32, frame_height: u32, cols: u32, rows: u32) -> Self {
        assert!(cols >= 1 && cols <= frame_width, "cols must be in [1, frame_width]");
        assert!(rows >= 1 && rows <= frame_height, "rows must be in [1, frame_height]");
        Self {
            cols,
            rows,
            frame_width,
            frame_height,
            cell_width: frame_width / cols,
            cell_height: frame_height / rows,
            num_cells: (cols * rows) as usize,
        }
    }

    /// Returns (x_start, x_end, y_start, y_end) for a cell by linear index.
    /// Last column/row absorbs remainder pixels for non-divisible dimensions.
    pub fn cell_bounds(&self, index: usize) -> (u32, u32, u32, u32) {
        let col = (index as u32) % self.cols;
        let row = (index as u32) / self.cols;
        let x_start = col * self.cell_width;
        let x_end = if col == self.cols - 1 { self.frame_width } else { (col + 1) * self.cell_width };
        let y_start = row * self.cell_height;
        let y_end = if row == self.rows - 1 { self.frame_height } else { (row + 1) * self.cell_height };
        (x_start, x_end, y_start, y_end)
    }
}

/// Compute per-cell mean absolute difference between two grayscale frames.
/// Results are written into `out` (length must equal `config.num_cells`), normalized to [0, 1].
/// Zero-allocation — caller owns the output buffer.
pub fn grid_intensities(prev: &[u8], curr: &[u8], config: &GridConfig, out: &mut [f64]) {
    let expected = (config.frame_width * config.frame_height) as usize;
    assert_eq!(prev.len(), expected);
    assert_eq!(curr.len(), expected);
    assert_eq!(out.len(), config.num_cells);

    let w = config.frame_width as usize;

    for (i, cell_out) in out.iter_mut().enumerate() {
        let (x0, x1, y0, y1) = config.cell_bounds(i);
        let mut sum: u64 = 0;
        let mut count: u64 = 0;

        for y in y0..y1 {
            let row_offset = y as usize * w;
            let start = row_offset + x0 as usize;
            let end = row_offset + x1 as usize;
            for j in start..end {
                sum += (prev[j] as i32 - curr[j] as i32).unsigned_abs() as u64;
                count += 1;
            }
        }

        *cell_out = if count > 0 {
            sum as f64 / (count as f64 * 255.0)
        } else {
            0.0
        };
    }
}

/// Info returned when grid-based motion is detected.
pub struct MotionEvent {
    /// Highest rolling-average cell intensity.
    pub max_cell_intensity: f64,
    /// Linear index of the hottest cell.
    pub max_cell_index: usize,
    /// Number of cells that exceeded their threshold.
    pub num_elevated_cells: usize,
}

/// Result of a single detector update.
pub enum DetectorResult {
    /// No elevated cells.
    None,
    /// Elevated cells detected but debounce not yet met.
    Debouncing,
    /// Motion confirmed after debounce.
    Motion(MotionEvent),
    /// Majority of cells elevated — camera jitter, lighting change, etc.
    FalsePositive { num_elevated_cells: usize },
}

impl DetectorResult {
    pub fn motion(self) -> Option<MotionEvent> {
        match self {
            DetectorResult::Motion(e) => Some(e),
            _ => None,
        }
    }
}

/// Grid-aware motion detector with per-cell rolling windows, debounce, and adaptive baselines.
pub struct GridMotionDetector {
    num_cells: usize,
    baselines: Vec<f64>,
    threshold_offset: f64,
    windows: Vec<VecDeque<f64>>,
    window_max: usize,
    debounce_count: usize,
    elevated_streak: usize,
    // Adaptive baseline EMA state
    ema_means: Vec<f64>,
    ema_variances: Vec<f64>,
    alpha: f64,
    adapt_frame_count: u64,
    warmup_frames: u64,
}

impl GridMotionDetector {
    /// Create a new grid detector.
    ///
    /// - `cell_stats`: per-cell `(mean, std_dev)` from calibration.
    /// - `adapt_tau`: EMA time constant in seconds (0 = no adaptation).
    pub fn new(
        cell_stats: Vec<(f64, f64)>,
        threshold_offset: f64,
        fps_estimate: f64,
        debounce_secs: f64,
        adapt_tau: f64,
    ) -> Self {
        let num_cells = cell_stats.len();
        let window_max = (fps_estimate * 0.15).max(1.0) as usize;
        let debounce_count = (fps_estimate * debounce_secs).max(1.0) as usize;
        let windows = (0..num_cells)
            .map(|_| VecDeque::with_capacity(window_max))
            .collect();

        let ema_means: Vec<f64> = cell_stats.iter().map(|&(m, _)| m).collect();
        let ema_variances: Vec<f64> = cell_stats.iter().map(|&(_, s)| s * s).collect();
        let baselines: Vec<f64> = cell_stats
            .iter()
            .map(|&(m, s)| m + 2.0 * s)
            .collect();

        let alpha = if adapt_tau > 0.0 {
            1.0 - (-1.0 / (fps_estimate * adapt_tau)).exp()
        } else {
            0.0
        };
        let warmup_frames = (fps_estimate * 5.0).max(1.0) as u64;

        Self {
            num_cells,
            baselines,
            threshold_offset,
            windows,
            window_max,
            debounce_count,
            elevated_streak: 0,
            ema_means,
            ema_variances,
            alpha,
            adapt_frame_count: 0,
            warmup_frames,
        }
    }

    /// Feed per-cell intensities and return the detection result.
    pub fn update(&mut self, cell_intensities: &[f64]) -> DetectorResult {
        assert_eq!(cell_intensities.len(), self.num_cells);

        let mut max_avg = 0.0f64;
        let mut max_idx = 0usize;
        let mut elevated_cells = 0usize;

        for (i, (&intensity, (win, baseline))) in cell_intensities
            .iter()
            .zip(self.windows.iter_mut().zip(self.baselines.iter()))
            .enumerate()
        {
            win.push_back(intensity);
            if win.len() > self.window_max {
                win.pop_front();
            }

            let rolling_avg = win.iter().sum::<f64>() / win.len() as f64;
            let threshold = baseline + self.threshold_offset;

            if rolling_avg > threshold {
                elevated_cells += 1;
            }
            if rolling_avg > max_avg {
                max_avg = rolling_avg;
                max_idx = i;
            }
        }

        // Majority of cells elevated — camera jitter, lighting change, IR toggle
        if elevated_cells * 2 > self.num_cells {
            self.elevated_streak = 0;
            return DetectorResult::FalsePositive { num_elevated_cells: elevated_cells };
        }

        if elevated_cells > 0 {
            self.elevated_streak += 1;
            if self.elevated_streak >= self.debounce_count {
                return DetectorResult::Motion(MotionEvent {
                    max_cell_intensity: max_avg,
                    max_cell_index: max_idx,
                    num_elevated_cells: elevated_cells,
                });
            }
            return DetectorResult::Debouncing;
        }

        self.elevated_streak = 0;
        self.adapt_baselines(cell_intensities);
        DetectorResult::None
    }

    /// Update per-cell baselines using EMA on non-motion frames.
    fn adapt_baselines(&mut self, cell_intensities: &[f64]) {
        if self.alpha == 0.0 {
            return;
        }
        self.adapt_frame_count += 1;
        if self.adapt_frame_count < self.warmup_frames {
            return;
        }
        let a = self.alpha;
        let one_minus_a = 1.0 - a;
        for ((&x, ema_m), (ema_v, bl)) in cell_intensities
            .iter()
            .zip(self.ema_means.iter_mut())
            .zip(self.ema_variances.iter_mut().zip(self.baselines.iter_mut()))
        {
            let diff = x - *ema_m;
            *ema_m = a * x + one_minus_a * *ema_m;
            *ema_v = (one_minus_a * (*ema_v + a * diff * diff)).max(0.0);
            *bl = *ema_m + 2.0 * ema_v.sqrt();
        }
    }

    #[cfg(test)]
    pub fn baselines(&self) -> &[f64] {
        &self.baselines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_identical_frames_all_zero() {
        let config = GridConfig::new(8, 6, 4, 3);
        let frame = vec![128u8; 48];
        let mut out = vec![0.0; config.num_cells];
        grid_intensities(&frame, &frame, &config, &mut out);
        for &v in &out {
            assert_eq!(v, 0.0);
        }
    }

    #[test]
    fn grid_localized_change() {
        // 8x6 frame, 4x3 grid → cells are 2x2 pixels each
        let config = GridConfig::new(8, 6, 4, 3);
        let prev = vec![0u8; 48];
        let mut curr = vec![0u8; 48];
        // Modify only pixels in cell (0,0) which covers x=[0,2) y=[0,2)
        // pixel (0,0) = index 0, pixel (1,0) = index 1
        // pixel (0,1) = index 8, pixel (1,1) = index 9
        curr[0] = 255;
        curr[1] = 255;
        curr[8] = 255;
        curr[9] = 255;

        let mut out = vec![0.0; config.num_cells];
        grid_intensities(&prev, &curr, &config, &mut out);

        // Cell 0 should have max intensity (4 pixels all 0→255)
        assert!((out[0] - 1.0).abs() < 1e-10);
        // All other cells should be 0
        for &v in &out[1..] {
            assert_eq!(v, 0.0);
        }
    }

    #[test]
    fn grid_non_divisible_dimensions() {
        // 10x7 frame, 3x2 grid → cell_width=3, cell_height=3
        // last col absorbs 10-3*3=1 extra px, last row absorbs 7-3*2=1 extra px
        let config = GridConfig::new(10, 7, 3, 2);
        assert_eq!(config.num_cells, 6);

        // Verify all pixels are covered exactly once
        let mut covered = vec![false; 70];
        for i in 0..config.num_cells {
            let (x0, x1, y0, y1) = config.cell_bounds(i);
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y * 10 + x) as usize;
                    assert!(!covered[idx], "pixel {idx} covered twice");
                    covered[idx] = true;
                }
            }
        }
        assert!(covered.iter().all(|&c| c), "not all pixels covered");
    }

    // Helper: create cell_stats with uniform mean and std_dev
    fn uniform_stats(mean: f64, std_dev: f64, n: usize) -> Vec<(f64, f64)> {
        vec![(mean, std_dev); n]
    }

    #[test]
    fn grid_detector_single_cell_triggers() {
        // baseline = 0.1, offset = 0.2, threshold = 0.3
        let stats = uniform_stats(0.05, 0.025, 4);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 0.0);
        let intensities = [0.0, 0.0, 0.5, 0.0];
        let mut detected = false;
        for _ in 0..10 {
            if det.update(&intensities).motion().is_some() {
                detected = true;
                break;
            }
        }
        assert!(detected);
    }

    #[test]
    fn grid_detector_no_motion_below_threshold() {
        // baseline = 0.1, offset = 0.2, threshold = 0.3
        let stats = uniform_stats(0.05, 0.025, 4);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 0.0);
        let intensities = [0.05, 0.05, 0.05, 0.05];
        for _ in 0..20 {
            assert!(det.update(&intensities).motion().is_none());
        }
    }

    // --- Adaptive baseline tests ---

    #[test]
    fn adapt_tau_zero_no_change() {
        let stats = uniform_stats(0.05, 0.025, 4);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 0.0);
        let original: Vec<f64> = det.baselines().to_vec();
        let intensities = [0.05, 0.05, 0.05, 0.05];
        for _ in 0..100 {
            det.update(&intensities);
        }
        assert_eq!(det.baselines(), &original[..]);
    }

    #[test]
    fn adapt_baselines_shift() {
        // Start with stats centered at 0.05, adapt toward 0.10
        let stats = uniform_stats(0.05, 0.01, 2);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 1.0);
        let original_b0 = det.baselines()[0];

        // Feed many non-motion frames at higher intensity
        let new_intensity = [0.10, 0.10];
        for _ in 0..500 {
            det.update(&new_intensity);
        }

        // Baselines should have shifted upward toward the new mean
        assert!(det.baselines()[0] > original_b0, "baseline should increase");
        // EMA mean should be close to 0.10
        assert!((det.baselines()[0] - 0.10).abs() < 0.02, "baseline should be near new level");
    }

    #[test]
    fn adapt_skipped_during_motion() {
        // baseline = 0.1, offset = 0.2, threshold = 0.3 → intensity 0.5 triggers
        let stats = uniform_stats(0.05, 0.025, 2);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 1.0);
        let original: Vec<f64> = det.baselines().to_vec();

        let high = [0.5, 0.5];
        for _ in 0..100 {
            det.update(&high);
        }

        assert_eq!(det.baselines(), &original[..]);
    }

    #[test]
    fn adapt_warmup_respected() {
        let stats = uniform_stats(0.05, 0.01, 2);
        let mut det = GridMotionDetector::new(stats, 0.2, 10.0, 0.3, 1.0);
        let original: Vec<f64> = det.baselines().to_vec();

        let intensities = [0.20, 0.20];
        for _ in 0..40 {
            det.update(&intensities);
        }

        assert_eq!(det.baselines(), &original[..]);
    }
}
