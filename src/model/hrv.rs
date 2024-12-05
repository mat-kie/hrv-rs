//! HRV (Heart Rate Variability) Model
//!
//! This module defines the data structures and methods for managing HRV data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics,
//! including calculations of RMSSD, SDRR, and Poincaré plot metrics.
//! The module processes raw heart rate data and computes various HRV parameters used
//! in the analysis of heart rate variability.

use super::bluetooth::HeartrateMessage;
use crate::math::hrv::{calc_poincare_metrics, calc_rmssd, calc_sdrr};
use anyhow::{anyhow, Result};
use nalgebra::DVector;
use std::fmt::Debug;
use time::Duration;

/// The size of the sliding window used in the outlier filter.
///
/// This constant defines the number of RR intervals considered when applying
/// the outlier filter to remove anomalies in the data.
const FILTER_WINDOW_SIZE: usize = 5;

/// Stores heart rate variability (HRV) statistics results.
///
/// This structure contains the calculated HRV parameters based on RR intervals.
/// It includes statistical measures like RMSSD, SDRR, and Poincaré plot metrics.
#[derive(Default, Clone, Debug)]
pub struct HrvStatistics {
    /// Root Mean Square of Successive Differences (RMSSD).
    pub rmssd: f64,
    /// Standard Deviation of RR intervals (SDRR).
    pub sdrr: f64,
    /// Short-term variability (SD1) from Poincaré plot.
    pub sd1: f64,
    /// Eigenvector corresponding to SD1.
    #[allow(dead_code)]
    pub sd1_eigenvec: [f64; 2],
    /// Long-term variability (SD2) from Poincaré plot.
    pub sd2: f64,
    /// Eigenvector corresponding to SD2.
    #[allow(dead_code)]
    pub sd2_eigenvec: [f64; 2],
    /// Ratio of SD1 to SD2, indicating the balance between short-term and long-term variability.
    #[allow(dead_code)]
    pub sd1_sd2_ratio: f64,
    /// Average heart rate over the analysis period.
    pub avg_hr: f64,
}

/// Manages runtime data related to HRV analysis.
///
/// This structure collects RR intervals, heart rate values, and timestamps.
/// It processes incoming heart rate measurements and computes HRV statistics.
#[derive(Default, Debug, Clone)]
pub struct HrvSessionData {
    /// RR intervals in milliseconds.
    pub rr_intervals: Vec<f64>,
    /// Cumulative time for each RR interval.
    pub rr_time: Vec<Duration>,
    /// Heart rate values.
    pub hr_values: Vec<f64>,
    /// Reception timestamps for heart rate measurements.
    pub rx_time: Vec<Duration>,
    /// Calculated HRV statistics.
    pub hrv_stats: Option<HrvStatistics>,
    /// Time series of RMSSD values over time.
    pub rmssd_ts: Vec<[f64; 2]>,
    /// Time series of SD1 values over time.
    pub sd1_ts: Vec<[f64; 2]>,
    /// Time series of SD2 values over time.
    pub sd2_ts: Vec<[f64; 2]>,
    /// Time series of heart rate values over time.
    pub hr_ts: Vec<[f64; 2]>,
}

/// Represents data collected during an HRV (Heart Rate Variability) session.
///
/// This struct holds heart rate values, RR intervals, reception timestamps, and HRV statistics.
/// It provides methods for processing raw acquisition data, filtering outliers, and calculating
/// HRV statistics.
impl HrvSessionData {
    /// Creates an `HrvSessionData` instance from acquisition data.
    ///
    /// Processes raw acquisition data, applies optional time-based filtering,
    /// filters outliers from the RR intervals, and calculates HRV statistics.
    ///
    /// # Arguments
    ///
    /// * `data` - A slice of `(Duration, HeartrateMessage)` tuples representing
    ///   time-stamped heart rate measurements.
    /// * `window` - An optional `Duration` specifying the time window for filtering data.
    ///   Only measurements within this window will be included.
    /// * `outlier_filter` - A threshold value used for identifying and removing outliers
    ///   in RR intervals.
    ///
    /// # Returns
    ///
    /// Returns an `Ok(HrvSessionData)` if the processing succeeds, or an `Err` if HRV
    /// statistics calculation fails (e.g., due to insufficient data).
    pub fn from_acquisition(
        data: &[(Duration, HeartrateMessage)],
        window: Option<Duration>,
        outlier_filter: f64,
    ) -> Result<Self> {
        let mut new = Self::default();
        if data.is_empty() {
            return Ok(new);
        }

        new.hr_values.reserve(data.len());
        new.rr_intervals.reserve(data.len());
        new.rx_time.reserve(data.len());

        let start_time = if let Some(window) = window {
            // we know the vector is not empty at this point
            data.last().unwrap().0 - window
        } else {
            // we know the vector is not empty at this point
            data.first().unwrap().0
        };

        for (ts, msg) in data.iter().filter(|val| val.0 >= start_time) {
            new.add_measurement(msg, ts);
        }

        if new.has_sufficient_data() {
            // Apply the outlier filter to the RR intervals and times.
            let (filtered_rr, filtered_time) = Self::apply_outlier_filter(
                &new.rr_intervals,
                Some(&new.rr_time),
                outlier_filter,
                FILTER_WINDOW_SIZE,
            );

            new.rr_intervals = filtered_rr;
            new.rr_time = filtered_time;

            let hrv_stats = HrvStatistics::new(&new.rr_intervals, &new.hr_values)?;
            let rr_total: Vec<f64> = data
                .iter()
                .flat_map(|m| m.1.get_rr_intervals())
                .map(|f| *f as f64)
                .collect();

            let ahr = hrv_stats.avg_hr;
            let win = (ahr * window.unwrap_or(Duration::seconds(30)).as_seconds_f64() / 60.0)
                .floor() as usize;

            let mut elapsed_time = 0.0;
            for (start_rr, rr) in rr_total
                .iter()
                .zip(rr_total.windows(win.max(1)).map(|slice| {
                    Self::apply_outlier_filter(slice, None, outlier_filter, FILTER_WINDOW_SIZE).0
                }))
            {
                let hr = 60000.0 * rr.len() as f64 / rr.iter().sum::<f64>();
                elapsed_time += start_rr * 1e-3;

                if let Ok(stats) = HrvStatistics::new(&rr, Default::default()) {
                    new.rmssd_ts.push([elapsed_time, stats.rmssd]);
                    new.sd1_ts.push([elapsed_time, stats.sd1]);
                    new.sd2_ts.push([elapsed_time, stats.sd2]);
                    new.hr_ts.push([elapsed_time, hr]);
                }
            }

            new.hrv_stats = Some(hrv_stats);
        }

        Ok(new)
    }

    /// Adds an RR interval measurement to the session.
    ///
    /// Calculates cumulative time and updates the `rr_intervals` and `rr_time` vectors.
    ///
    /// # Arguments
    ///
    /// * `rr_measurement` - The RR interval in milliseconds.
    fn add_rr_measurement(&mut self, rr_measurement: u16) {
        let rr_ms = rr_measurement as f64;
        let cumulative_time = if let Some(last) = self.rr_time.last() {
            *last + Duration::milliseconds(rr_measurement as i64)
        } else {
            Duration::milliseconds(rr_measurement as i64)
        };

        self.rr_intervals.push(rr_ms);
        self.rr_time.push(cumulative_time);
    }

    /// Adds a heart rate measurement to the session data.
    ///
    /// Updates the session with RR intervals, heart rate values, and reception timestamps
    /// extracted from the provided `HeartrateMessage`.
    ///
    /// # Arguments
    ///
    /// * `hrs_msg` - The `HeartrateMessage` containing HR and RR interval data.
    /// * `elapsed_time` - The timestamp associated with the message.
    fn add_measurement(&mut self, hrs_msg: &HeartrateMessage, elapsed_time: &Duration) {
        for &rr_interval in hrs_msg.get_rr_intervals() {
            self.add_rr_measurement(rr_interval);
        }
        self.hr_values.push(hrs_msg.get_hr());
        self.rx_time.push(*elapsed_time);
    }

    /// Returns a list of Poincaré plot points.
    ///
    /// # Returns
    ///
    /// A vector of `[f64; 2]` points representing successive RR intervals.
    pub fn get_poincare(&self) -> Vec<[f64; 2]> {
        self.rr_intervals
            .windows(2)
            .map(|win| [win[0], win[1]])
            .collect()
    }

    /// Checks if there is sufficient data for HRV calculations.
    ///
    /// # Returns
    ///
    /// `true` if there are enough RR intervals to perform HRV analysis; `false` otherwise.
    pub fn has_sufficient_data(&self) -> bool {
        self.rr_intervals.len() >= 4
    }

    /// Applies an outlier filter to the RR intervals and optional time series.
    ///
    /// # Arguments
    ///
    /// * `rr_intervals` - A slice of RR intervals to filter.
    /// * `opt_rr_time` - An optional slice of timestamps corresponding to the RR intervals.
    /// * `outlier_filter` - The outlier threshold for filtering.
    /// * `window_size` - The size of the sliding window used for filtering.
    ///
    /// # Returns
    ///
    /// A tuple `(Vec<f64>, Vec<Duration>)` containing the filtered RR intervals
    /// and timestamps (empty if `opt_rr_time` is `None`).
    fn apply_outlier_filter(
        rr_intervals: &[f64],
        opt_rr_time: Option<&[Duration]>,
        outlier_filter: f64,
        window_size: usize,
    ) -> (Vec<f64>, Vec<Duration>) {
        let half_window = window_size / 2;

        // Helper function to check if a value is an outlier
        let is_outlier = |idx: usize, values: &[f64]| {
            let mut start = idx.saturating_sub(half_window);
            let mut end = start + window_size;
            if end >= values.len() {
                end = values.len();
                start = end.saturating_sub(window_size);
            }

            let window = &values[start..end];
            let mean = window
                .iter()
                .enumerate()
                .filter(|(i, _)| start + i != idx)
                .map(|(_, &v)| v)
                .sum::<f64>()
                / (window.len() - 1) as f64;

            let deviation = (values[idx] - mean).abs();

            deviation > outlier_filter
        };

        if let Some(rr_time) = opt_rr_time {
            // Process both RR intervals and timestamps
            rr_intervals
                .iter()
                .zip(rr_time)
                .enumerate()
                .filter_map(|(i, (&rr, &time))| {
                    if !is_outlier(i, rr_intervals) {
                        Some((rr, time))
                    } else {
                        None
                    }
                })
                .unzip()
        } else {
            // Process only RR intervals
            (
                rr_intervals
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &rr)| {
                        if !is_outlier(i, rr_intervals) {
                            Some(rr)
                        } else {
                            None
                        }
                    })
                    .collect(),
                Default::default(),
            )
        }
    }
}

impl HrvStatistics {
    /// Constructs a new `HrvStatistics` from RR intervals and heart rate values.
    ///
    /// # Arguments
    ///
    /// * `rr_intervals` - A slice of RR intervals in milliseconds.
    /// * `hr_values` - A slice of heart rate values.
    ///
    /// # Returns
    ///
    /// Returns an `Ok(HrvStatistics)` containing the calculated HRV statistics, or
    /// an `Err` if there is insufficient data.
    fn new(rr_intervals: &[f64], hr_values: &[f64]) -> Result<Self> {
        if rr_intervals.len() < 4 {
            return Err(anyhow!(
                "Not enough RR intervals for HRV stats calculation."
            ));
        }

        let avg_hr = if hr_values.is_empty() {
            0.0
        } else {
            DVector::from_row_slice(hr_values).mean()
        };

        let poincare = calc_poincare_metrics(rr_intervals);

        Ok(HrvStatistics {
            rmssd: calc_rmssd(rr_intervals),
            sdrr: calc_sdrr(rr_intervals),
            sd1: poincare.sd1,
            sd1_eigenvec: poincare.sd1_eigenvector,
            sd2: poincare.sd2,
            sd2_eigenvec: poincare.sd2_eigenvector,
            sd1_sd2_ratio: poincare.sd1 / poincare.sd2,
            avg_hr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hrv_runtime_data_add_measurement() {
        let mut runtime = HrvSessionData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        assert!(!runtime.has_sufficient_data());
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        assert!(runtime.has_sufficient_data());
    }

    #[test]
    fn test_hrv_statistics_new() {
        let rr_intervals = vec![800.0, 810.0, 790.0, 805.0];
        let hr_values = vec![75.0, 76.0, 74.0, 75.5];
        let hrv_stats = HrvStatistics::new(&rr_intervals, &hr_values).unwrap();
        assert!(hrv_stats.rmssd > 0.0);
        assert!(hrv_stats.sdrr > 0.0);
        assert!(hrv_stats.sd1 > 0.0);
        assert!(hrv_stats.sd2 > 0.0);
        assert!(hrv_stats.avg_hr > 0.0);
    }

    #[test]
    fn test_hrv_session_data_from_acquisition() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let data = vec![
            (Duration::milliseconds(0), hr_msg),
            (Duration::milliseconds(1000), hr_msg),
            (Duration::milliseconds(2000), hr_msg),
            (Duration::milliseconds(3000), hr_msg),
        ];
        let session_data = HrvSessionData::from_acquisition(&data, None, 50.0).unwrap();
        assert!(session_data.has_sufficient_data());
        assert!(session_data.hrv_stats.is_some());
    }

    #[test]
    fn test_apply_outlier_filter() {
        let rr_intervals = vec![800.0, 810.0, 790.0, 805.0, 900.0, 805.0, 810.0];
        let (filtered_rr, _) = HrvSessionData::apply_outlier_filter(&rr_intervals, None, 50.0, 5);
        assert_eq!(filtered_rr.len(), 6); // The outlier (900.0) should be filtered out
    }

    #[test]
    fn test_get_poincare() {
        let session_data = HrvSessionData {
            rr_intervals: vec![800.0, 810.0, 790.0, 805.0],
            ..Default::default()
        };
        let poincare_points = session_data.get_poincare();
        assert_eq!(poincare_points.len(), 3);
        assert_eq!(poincare_points[0], [800.0, 810.0]);
        assert_eq!(poincare_points[1], [810.0, 790.0]);
        assert_eq!(poincare_points[2], [790.0, 805.0]);
    }
}
