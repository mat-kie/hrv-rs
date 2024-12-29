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
use hrv_algos::preprocessing::outliers::{self, classify_rr_values, OutlierType};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::fmt::Debug;
use std::mem::swap;
use std::usize;
use time::Duration;

#[derive(Clone, Debug, Default)]
struct RuntimeRecordingData {
    /// RR intervals in milliseconds.
    rr_intervals: Vec<f64>,
    /// RR interval calssification.
    rr_classification: Vec<OutlierType>,
    /// Cumulative time for each RR interval.
    rr_time: Vec<Duration>,
    outlier_filter: f64,
}
impl RuntimeRecordingData {
    pub fn new(rr_data: Vec<f64>, outlier_filter: f64) -> Self {
        let mut new = Self {
            outlier_filter,
            ..Default::default()
        };
        new.add_rr(&rr_data);
        new
    }

    pub fn add_rr(&mut self, rr: &[f64]) -> Result<()> {
        {
            let mut new_ts: Vec<_> = rr
                .iter()
                .scan(
                    *self.rr_time.last().unwrap_or(&Duration::default()),
                    |acc, &rr| {
                        *acc += Duration::milliseconds(rr as i64);
                        Some(*acc)
                    },
                )
                .collect();
            self.rr_time.append(&mut new_ts);
        }
       self.update_classification()?;
        Ok(())
    }

    pub fn update_classification(&mut self)->Result<()>{
        const ANALYSIS_WINDOW: usize = 91;
        // classification uses a 91 item rolling quantile
        // take 91 last rr, update the last 46 elements classification
        let win_start = self.rr_classification.len().saturating_sub(ANALYSIS_WINDOW);
        let cutoff = self
            .rr_classification
            .len()
            .saturating_sub(ANALYSIS_WINDOW / 2);
        let added_rr = self.rr_intervals.len().saturating_sub(self.rr_classification.len());
        let data = &self.rr_intervals[win_start..];
        let new_class = classify_rr_values(data, None, None, Some(self.outlier_filter))?;
        let mut added_classes = new_class[new_class.len().saturating_sub(added_rr)..].to_vec();
        self.rr_classification.append(&mut added_classes);
        //  update the last 46 elements classification
        self.rr_classification
            .iter_mut()
            .skip(cutoff)
            .zip(
                new_class
                    .iter()
                    .skip(new_class.len().saturating_sub(ANALYSIS_WINDOW / 2)),
            )
            .map(|(a, b)| *a = *b);
        Ok(())

    }

    pub fn set_filter(&mut self, filter: f64)->Result<()>
    {
        if filter != self.outlier_filter{
            let mut old = Vec::default();
            swap(&mut old,&mut self.rr_classification);
            let res = self.update_classification();
            if res.is_err(){
                self.rr_classification = old;
                return res
            }else{
                return Ok(())
            }
            
        }
        Ok(())
    }

    pub fn get_rr(&self) -> &[f64] {
        return &self.rr_intervals;
    }
    pub fn get_classification(&self) -> &[OutlierType]{
        &self.rr_classification
    }

    pub fn get_ts(&self)->&[Duration]{
        &self.rr_time
    }

    fn get_filtered<T: Send + Sync + Clone>(&self, data: &[T]) -> Vec<T> {
        data.par_iter()
            .zip(&self.rr_classification)
            .filter_map(|(rr, class)| {
                if matches!(class, OutlierType::None) {
                    Some(rr.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_filtered_rr(&self) -> Vec<f64> {
        self.get_filtered(&self.rr_intervals)
    }
    pub fn get_filtered_ts(&self) -> Vec<Duration> {
        self.get_filtered(&self.rr_time)
    }
    pub fn get_poincare(&self, samples: Option<usize>) -> Result<(Vec<[f64; 2]>, Vec<[f64; 2]>)> {
        if self.rr_intervals.len() < 2 {
            return Err(anyhow!("too few rr intervals for poincare points"));
        }
        let start = samples
            .map(|s| self.rr_intervals.len().saturating_sub(s))
            .unwrap_or(0);
        let mut inliers = Vec::with_capacity(samples.unwrap_or(self.rr_intervals.len()));
        let mut outliers = Vec::with_capacity(samples.unwrap_or(self.rr_intervals.len()));
        for (rr, classes) in self
            .rr_intervals
            .windows(2)
            .zip(self.rr_classification.windows(2))
            .skip(start)
        {
            if classes[0].is_outlier() || classes[1].is_outlier() {
                outliers.push([rr[0], rr[1]]);
            } else {
                inliers.push([rr[0], rr[1]]);
            }
        }
        inliers.shrink_to_fit();
        outliers.shrink_to_fit();

        Ok((inliers, outliers))
    }
}

/// Manages runtime data related to HRV analysis.
///
/// This structure collects RR intervals, heart rate values, and timestamps.
/// It processes incoming heart rate measurements and computes HRV statistics.
#[derive(Default, Debug, Clone)]
pub struct HrvSessionData {
    data: RuntimeRecordingData,
    /// Time series of RMSSD values.
    rmssd_ts: Vec<[f64; 2]>,
    /// Time series of SDRR values.
    sdrr_ts: Vec<[f64; 2]>,
    /// Time series of SD1 values.
    sd1_ts: Vec<[f64; 2]>,
    /// Time series of SD2 values.
    sd2_ts: Vec<[f64; 2]>,
    /// Time series of heart rate values.
    hr_ts: Vec<[f64; 2]>,
    /// Time series of DFA alpha values
    dfa_alpha_ts: Vec<[f64; 2]>,
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
            let filtered_rr = Self::remove_outliers(&new.rr_intervals, &new.rr_classification)?;
            let filtered_ts = Self::remove_outliers(&new.rr_time, &new.rr_classification)?;
            new.rmssd_ts = Self::calc_time_series(
                window.unwrap_or(usize::MAX),
                &filtered_ts,
                &filtered_rr,
                |ts, win| Ok([ts.as_seconds_f64(), calc_rmssd(win)?]),
            );
            new.sdrr_ts = Self::calc_time_series(
                window.unwrap_or(usize::MAX),
                &filtered_ts,
                &filtered_rr,
                |ts, win| Ok([ts.as_seconds_f64(), calc_sdrr(win)?]),
            );
            (new.sd1_ts, new.sd2_ts) = Self::calc_time_series(
                window.unwrap_or(usize::MAX),
                &filtered_ts,
                &filtered_rr,
                |ts, win| {
                    let res = calc_poincare_metrics(win)?;
                    let fsecs = ts.as_seconds_f64();
                    Ok(([fsecs, res.sd1], [fsecs, res.sd2]))
                },
            )
            .into_iter()
            .unzip();
            new.hr_ts = Self::calc_time_series(
                window.unwrap_or(usize::MAX),
                &filtered_ts,
                &filtered_rr,
                |ts, rr| {
                    Ok([
                        ts.as_seconds_f64(),
                        60000.0 * rr.len() as f64 / rr.iter().sum::<f64>(),
                    ])
                },
            );
        }

        Ok(new)
    }

    fn calc_time_series<
        'a,
        T: Send + Sync + 'a,
        R: Send + Sync,
        F: Fn(&Duration, &[T]) -> Result<R> + Send + Sync,
    >(
        window: usize,
        ts: &[Duration],
        data: &[T],
        func: F,
    ) -> Vec<R> {
        ts.par_iter()
            .enumerate()
            .filter_map(|(idx, ts)| {
                let rr = &data[idx.saturating_sub(window)..idx + 1];
                func(ts, rr).ok()
            })
            .collect()
    }

    fn remove_outliers<'a, T: Clone + Send + 'a>(
        data: &'a [T],
        labels: &[OutlierType],
    ) -> Result<Vec<T>>
    where
        [T]: IntoParallelRefIterator<'a>,
        <[T] as IntoParallelRefIterator<'a>>::Iter: IndexedParallelIterator<Item = &'a T>,
    {
        Ok(data
            .par_iter()
            .zip(labels)
            .filter_map(|(ts, class)| {
                if let OutlierType::None = class {
                    Some(ts.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<T>>())
    }

    pub fn add_measurement(
        &mut self,
        hrs_msg: &HeartrateMessage,
        window: usize,
        filter: f64,
    ) -> Result<()> {
        // add rr point
        let mut new_rr: Vec<f64> = hrs_msg
            .get_rr_intervals()
            .iter()
            .map(|&rr| f64::from(rr))
            .collect();
        let new_rr_len = new_rr.len();
        if new_rr.is_empty() {
            return Ok(());
        }
        let mut new_ts = new_rr
            .iter()
            .scan(
                *self.rr_time.last().unwrap_or(&Duration::default()),
                |acc, &rr| {
                    *acc += Duration::milliseconds(rr as i64);
                    Some(*acc)
                },
            )
            .collect();
        self.rr_intervals.append(&mut new_rr);
        // add elapsed time
        self.rr_time.append(&mut new_ts);

        let current_window = self
            .rr_intervals
            .windows(window.min(self.rr_intervals.len()))
            .last()
            .ok_or_else(|| anyhow!("Not enough data for window"))?;
        let rr = hide_quantization(current_window, None)?;
        // run outlier filter with window
        let class = classify_rr_values(&rr, None, None, Some(filter))?;
        let mut added = class
            .windows(new_rr_len)
            .last()
            .ok_or_else(|| anyhow!("classifier window failed"))?
            .to_vec();
        // add rr point classification
        self.rr_classification.append(&mut added);

        let range =
            self.rr_classification.len().saturating_sub(class.len())..self.rr_classification.len();
        self.rr_classification[range]
            .iter_mut()
            .zip(&class)
            .for_each(|(old, &new)| {
                *old = new;
            });
        let filtered_rr = rr
            .iter()
            .zip(&class)
            .filter_map(|(rr, class)| {
                if let OutlierType::None = class {
                    Some(*rr)
                } else {
                    None
                }
            })
            .collect::<Vec<f64>>();
        // calculate ts metrics with window and append to ts
        let rmssd = calc_rmssd(&filtered_rr)?;
        let sdrr = calc_sdrr(&filtered_rr)?;
        let poincare = calc_poincare_metrics(&filtered_rr)?;
        let hr = hrs_msg.get_hr();
        let elapsed_time = self.rr_time.last().unwrap().as_seconds_f64();

        self.rmssd_ts.push([elapsed_time, rmssd]);
        self.sdrr_ts.push([elapsed_time, sdrr]);
        self.sd1_ts.push([elapsed_time, poincare.sd1]);
        self.sd2_ts.push([elapsed_time, poincare.sd2]);
        self.hr_ts.push([elapsed_time, hr]);
        let dfa = DFAnalysis::udfa(
            &filtered_rr,
            &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            DetrendStrategy::Linear,
        )?;
        self.dfa_alpha_ts.push([elapsed_time, dfa.alpha]);

        Ok(())
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
    }

   /* #[test]
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
    } */
}
