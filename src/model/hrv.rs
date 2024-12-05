//! HRV Model
//!
//! This module defines the data structures and methods for managing HRV (Heart Rate Variability) data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics.

use super::bluetooth::HeartrateMessage;
use crate::math::hrv::{calc_poincare_metrics, calc_rmssd, calc_sdrr};
use anyhow::{anyhow, Result};
use nalgebra::DVector;
use std::fmt::Debug;
use time::Duration;

// TODO: make configurable
/// Constant for the outlier filter window size
const FILTER_WINDOW_SIZE: usize = 5;

/// Stores heart rate variability (HRV) statistics results.
#[derive(Default, Clone, Debug)]
/// `HrvStatistics` structure.
///
/// Represents data related to HRV analysis, including statistics and session details.
pub struct HrvStatistics {
    /// Root Mean Square of Successive Differences.
    pub rmssd: f64,
    /// Standard Deviation of RR intervals.
    #[allow(dead_code)]
    pub sdrr: f64,
    /// Poincare SD1 (short-term HRV).
    pub sd1: f64,
    /// Eigenvector for SD1.
    #[allow(dead_code)]
    pub sd1_eigenvec: [f64; 2],
    /// Poincare SD2 (long-term HRV).
    pub sd2: f64,
    /// Eigenvector for SD2.
    #[allow(dead_code)]
    pub sd2_eigenvec: [f64; 2],
    /// Ratio of SD1 to SD2.
    #[allow(dead_code)]
    pub sd1_sd2_ratio: f64,
    /// Average heart rate.
    pub avg_hr: f64,
}

/// `HrvSessionData` structure.
///
/// Manages runtime data related to HRV analysis, including RR intervals, heart rate values,
/// and the calculated HRV statistics.
#[derive(Default, Debug, Clone)]
pub struct HrvSessionData {
    /// RR intervals in milliseconds.
    pub rr_intervals: Vec<f64>,
    /// Cumulative time for each RR interval.
    pub rr_time: Vec<Duration>,
    /// Heart rate values.
    pub hr_values: Vec<f64>,
    /// Reception timestamps.
    pub rx_time: Vec<Duration>,
    /// Calculated HRV statistics.
    pub hrv_stats: Option<HrvStatistics>,
    pub rmssd_ts: Vec<[f64; 2]>,
    pub sd1_ts: Vec<[f64; 2]>,
    pub sd2_ts: Vec<[f64; 2]>,
    pub hr_ts: Vec<[f64; 2]>,
}

impl HrvSessionData {
    /// Creates an `HrvSessionData` instance from acquisition data.
    ///
    /// This method processes raw acquisition data, applies optional time-based filtering,
    /// filters outliers from the RR intervals, and calculates HRV statistics.
    ///
    /// # Parameters
    /// - `data`: A slice of `(Duration, HeartrateMessage)` tuples representing
    ///   time-stamped heart rate measurements.
    /// - `window`: An optional `Duration` specifying the time window for filtering data.
    ///   Only measurements after `data.last().unwrap().0 - window` will be included.
    /// - `outlier_filter`: A threshold value used for identifying and removing outliers
    ///   in RR intervals.
    ///
    /// # Returns
    /// - `Ok(HrvSessionData)` if the processing succeeds.
    /// - `Err` if HRV statistics calculation fails (e.g., insufficient data for statistics).
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
    /// # Parameters
    /// - `rr_measurement`: The RR interval in milliseconds.
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

    /// Adds an HR service message to the runtime data.
    ///
    /// Updates the session with RR intervals, heart rate values, and reception timestamps
    /// extracted from the provided `HeartrateMessage`.
    ///
    /// # Parameters
    /// - `hrs_msg`: The `HeartrateMessage` containing HR and RR interval data.
    /// - `elapsed_time`: The timestamp associated with the message.
    fn add_measurement(&mut self, hrs_msg: &HeartrateMessage, elapsed_time: &Duration) {
        for &rr_interval in hrs_msg.get_rr_intervals() {
            self.add_rr_measurement(rr_interval);
        }
        self.hr_values.push(hrs_msg.get_hr());
        self.rx_time.push(*elapsed_time);
    }

    /// Returns a list of Poincare plot points.
    ///
    /// # Returns
    /// A `Vec` of `[f64; 2]` points representing successive RR intervals.
    pub fn get_poincare(&self) -> Vec<[f64; 2]> {
        self.rr_intervals
            .windows(2)
            .map(|win| [win[0], win[1]])
            .collect()
    }

    /// Checks if there is sufficient data for HRV calculations.
    ///
    /// # Returns
    /// `true` if there are at least 4 RR intervals; `false` otherwise.
    pub fn has_sufficient_data(&self) -> bool {
        self.rr_intervals.len() >= 4
    }

    /// Applies an outlier filter to the RR intervals and optional time series.
    ///
    /// # Parameters
    /// - `rr_intervals`: A slice of RR intervals to filter.
    /// - `opt_rr_time`: An optional slice of timestamps corresponding to the RR intervals.
    /// - `outlier_filter`: The outlier threshold for filtering.
    /// - `window_size`: The size of the sliding window used for filtering.
    ///
    /// # Returns
    /// - A tuple `(Vec<f64>, Vec<Duration>)` containing the filtered RR intervals
    ///   and timestamps (empty if `opt_rr_time` is `None`).
    fn apply_outlier_filter(
        rr_intervals: &[f64],
        opt_rr_time: Option<&[Duration]>,
        outlier_filter: f64,
        window_size: usize,
    ) -> (Vec<f64>, Vec<Duration>) {
        let predicate = |rr_window: &[f64], window_size: usize| {
            let median_rr = rr_window[window_size / 2];
            let mean_rr = rr_window.iter().sum::<f64>() / window_size as f64;
            let deviation = (median_rr - mean_rr).abs() * 0.5;

            deviation < outlier_filter
        };
        if let Some(rr_time) = opt_rr_time {
            rr_intervals
                .windows(window_size)
                .zip(rr_time.windows(window_size))
                .filter_map(|(rr_window, time_window)| {
                    if predicate(rr_window, window_size) {
                        Some((rr_window[window_size / 2], time_window[window_size / 2]))
                    } else {
                        None
                    }
                })
                .unzip()
        } else {
            (
                rr_intervals
                    .windows(window_size)
                    .filter_map(|rr_window| {
                        if predicate(rr_window, window_size) {
                            Some(rr_window[window_size / 2])
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
    /// Constructs a new `HrvStatistics` from runtime data and an optional time window.
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
}
