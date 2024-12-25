//! HRV (Heart Rate Variability) Model
//!
//! This module defines the data structures and methods for managing HRV data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics,
//! including calculations of RMSSD, SDRR, and Poincaré plot metrics.
//! The module processes raw heart rate data and computes various HRV parameters used
//! in the analysis of heart rate variability.

use super::bluetooth::HeartrateMessage;
use anyhow::{anyhow, Result};
use hrv_algos::analysis::dfa::{DFAnalysis, DetrendStrategy};
use hrv_algos::analysis::nonlinear::calc_poincare_metrics;
use hrv_algos::analysis::time::{calc_rmssd, calc_sdrr};
use hrv_algos::preprocessing::noise::hide_quantization;
use hrv_algos::preprocessing::outliers::{classify_rr_values, OutlierType};

use nalgebra::DVector;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::fmt::Debug;
use time::Duration;

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
    /// RR interval calssification.
    pub rr_classification: Vec<OutlierType>,
    /// Cumulative time for each RR interval.
    pub rr_time: Vec<Duration>,
    /// Heart rate values.
    pub hr_values: Vec<f64>,
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
    pub dfa_alpha_ts: Vec<[f64; 2]>,
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
        window: Option<usize>,
        outlier_filter: f64,
    ) -> Result<Self> {
        let mut new = Self::default();
        if data.is_empty() {
            return Ok(new);
        }
        new.add_measurements(data);
        new.rr_intervals = hide_quantization(&new.rr_intervals, None)?;

        if new.has_sufficient_data() {
            // Apply the outlier filter to the RR intervals and times.
            new.rr_classification =
                classify_rr_values(&new.rr_intervals, None, None, Some(outlier_filter))?;
            let filtered_rr = new
                .rr_intervals
                .iter()
                .zip(&new.rr_classification)
                .filter_map(|(rr, class)| {
                    if let OutlierType::None = class {
                        Some(*rr)
                    } else {
                        None
                    }
                })
                .collect::<Vec<f64>>();
            let filtered_ts = new
                .rr_time
                .par_iter()
                .zip(&new.rr_classification)
                .filter_map(|(ts, class)| {
                    if let OutlierType::None = class {
                        Some(*ts)
                    } else {
                        None
                    }
                })
                .collect::<Vec<Duration>>();

            let hrv_stats = HrvStatistics::new(&filtered_rr, &new.hr_values)?;

            for (start_time, rr) in filtered_ts.iter().zip(
                filtered_rr
                    .windows(window.unwrap_or(usize::MAX).max(2))
                    .collect::<Vec<_>>(),
            ) {
                let hr = 60000.0 * rr.len() as f64 / rr.iter().sum::<f64>();
                let elapsed_time = start_time.as_seconds_f64();
                if let Ok(stats) = HrvStatistics::new(rr, Default::default()) {
                    new.rmssd_ts.push([elapsed_time, stats.rmssd]);
                    new.sd1_ts.push([elapsed_time, stats.sd1]);
                    new.sd2_ts.push([elapsed_time, stats.sd2]);
                    new.hr_ts.push([elapsed_time, hr]);
                    let windows: Vec<usize> = (4..17).collect();
                    if let Ok(analysis) = DFAnalysis::udfa(rr, &windows, DetrendStrategy::Linear) {
                        new.dfa_alpha_ts.push([elapsed_time, analysis.alpha]);
                    }
                }
            }

            new.hrv_stats = Some(hrv_stats);
        }

        Ok(new)
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
    fn add_measurements(&mut self, hrs_msgs: &[(Duration, HeartrateMessage)]) {
        self.rr_intervals = hrs_msgs
            .par_iter()
            .map(|(_, hrs_msg)| {
                hrs_msg
                    .get_rr_intervals()
                    .iter()
                    .filter_map(|&rr| if rr > 0 { Some(f64::from(rr)) } else { None })
                    .collect::<Vec<f64>>()
            })
            .flatten()
            .collect();
        self.hr_values = hrs_msgs
            .par_iter()
            .map(|(_, hrs_msg)| hrs_msg.get_hr())
            .collect();
        self.rr_time = self
            .rr_intervals
            .iter()
            .scan(Duration::default(), |acc, &rr| {
                *acc += Duration::milliseconds(rr as i64);
                Some(*acc)
            })
            .collect();
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

        let poincare = calc_poincare_metrics(rr_intervals)?;

        Ok(HrvStatistics {
            rmssd: calc_rmssd(rr_intervals)?,
            sdrr: calc_sdrr(rr_intervals)?,
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
        let data = [
            (Duration::milliseconds(0), hr_msg),
            (Duration::milliseconds(1000), hr_msg),
            (Duration::milliseconds(2000), hr_msg),
            (Duration::milliseconds(3000), hr_msg),
        ];
        runtime.add_measurements(&data[0..1]);
        assert!(!runtime.has_sufficient_data());
        runtime.add_measurements(&data[1..]);
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
