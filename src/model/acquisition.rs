//! Acquisition Model
//!
//! This module defines the model for managing acquisition data in the HRV analysis tool.
//! It provides structures and traits for handling real-time and stored data related to HRV.

use super::bluetooth::HeartrateMessage;
use crate::model::hrv::{HrvSessionData, HrvStatistics};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use time::{Duration, OffsetDateTime};

/// `AcquisitionModelApi` trait.
///
/// Defines the interface for managing acquisition-related data, including runtime measurements,
/// HRV statistics, and stored acquisitions.
pub trait AcquisitionModelApi: Debug + Send {
    /// Retrieves the start time of the current acquisition.
    ///
    /// # Returns
    /// An optional `OffsetDateTime` indicating the start time, if available.
    #[allow(dead_code)]
    fn get_start_time(&self) -> Option<OffsetDateTime>;

    /// Retrieves the last heart rate message received.
    ///
    /// # Returns
    /// An optional `HeartrateMessage` representing the most recent measurement.
    #[allow(dead_code)]
    fn get_last_msg(&self) -> Option<HeartrateMessage>;

    /// Retrieves the current HRV statistics.
    ///
    /// # Returns
    /// A reference to an optional `HrvStatistics` containing computed HRV data.
    fn get_hrv_stats(&self) -> &Option<HrvStatistics>;

    /// Retrieves the configured statistics window.
    ///
    /// # Returns
    /// A reference to an optional `Duration` representing the analysis window size.
    fn get_stats_window(&self) -> &Option<Duration>;

    /// Getter for the filter parameter value (fraction of std. dev)
    ///
    /// # Returns
    /// The parameter value for the outlier filter
    fn get_outlier_filter_value(&self) -> f64;

    /// Setter for the filter parameter value (fraction of std. dev)
    fn set_outlier_filter_value(&mut self, value: f64);

    /// Retrieves the points for the Poincare plot.
    ///
    /// # Returns
    /// A vector of `[f64; 2]` pairs representing the Poincare points.
    fn get_poincare_points(&self) -> Vec<[f64; 2]>;

    /// Adds a new heart rate measurement to the current acquisition.
    ///
    /// # Arguments
    /// - `msg`: The `HeartrateMessage` containing the measurement data.
    fn add_measurement(&mut self, msg: &HeartrateMessage);

    /// Sets the statistics analysis window.
    ///
    /// # Arguments
    /// - `window`: A `Duration` representing the new analysis window size.
    fn set_stats_window(&mut self, window: &Duration);

    /// Stores the current acquisition, moving it to the stored data list.
    fn store_acquisition(&mut self);

    /// Discards the current acquisition and clears associated data.
    fn discard_acquisition(&mut self);
}

/// Holds the model's stored data, including athlete information and measurements.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ModelData {
    measurements: Vec<Acquisition>,
}

/// Represents the acquisition model, managing HRV-related data and operations.
#[derive(Debug, Default)]
pub struct AcquisitionModel {
    data: ModelData,
    rt_data: HrvSessionData,
    active_acq: Option<Acquisition>,
}

impl AcquisitionModelApi for AcquisitionModel {
    fn get_start_time(&self) -> Option<OffsetDateTime> {
        self.active_acq.as_ref().map(|acq| acq.start_time)
    }

    fn get_last_msg(&self) -> Option<HeartrateMessage> {
        self.active_acq
            .as_ref()
            .and_then(|acq| acq.measurements.last().map(|(_, msg)| *msg))
    }

    fn get_hrv_stats(&self) -> &Option<HrvStatistics> {
        &self.rt_data.hrv_stats
    }

    fn get_stats_window(&self) -> &Option<Duration> {
        &self.rt_data.stats_window
    }

    fn get_poincare_points(&self) -> Vec<[f64; 2]> {
        self.rt_data.get_poincare()
    }

    fn add_measurement(&mut self, msg: &HeartrateMessage) {
        if let Some(acq) = self.active_acq.as_mut() {
            acq.add_measurement(*msg);
        } else {
            let mut acq = Acquisition::new();
            acq.add_measurement(*msg);
            self.active_acq = Some(acq);
        }
        if let Some(acq) = self.active_acq.as_mut() {
            let elapsed = acq.get_measurements().last().unwrap().0;
            self.rt_data.add_measurement(msg, &elapsed);
            self.rt_data.update_stats();
        }
    }

    fn set_stats_window(&mut self, window: &Duration) {
        self.rt_data.set_stats_window(Some(*window));
    }
    fn get_outlier_filter_value(&self) -> f64 {
        self.rt_data.filter_value
    }
    fn set_outlier_filter_value(&mut self, value: f64) {
        if value >= 0.0 {
            self.rt_data.filter_value = value
        }
    }

    fn store_acquisition(&mut self) {
        if let Some(acq) = self.active_acq.take() {
            self.data.measurements.push(acq);
        }
    }

    fn discard_acquisition(&mut self) {
        self.active_acq = None;
    }
}

/// Represents acquisition data for heart rate measurements.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Acquisition {
    /// The start time of the acquisition.
    start_time: OffsetDateTime,
    /// Collected measurements with their elapsed time.
    measurements: Vec<(Duration, HeartrateMessage)>,
}

impl Acquisition {
    pub fn new() -> Self {
        Self {
            start_time: OffsetDateTime::now_utc(),
            measurements: Vec::new(),
        }
    }

    pub fn get_measurements(&self) -> &[(Duration, HeartrateMessage)] {
        &self.measurements
    }

    pub fn add_measurement(&mut self, measurement: HeartrateMessage) {
        let elapsed = OffsetDateTime::now_utc() - self.start_time;
        self.measurements.push((elapsed, measurement));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acquisition_initialization() {
        let acquisition = Acquisition::new();
        assert!(acquisition.get_measurements().is_empty());
    }

    #[test]
    fn test_add_measurement() {
        let mut acquisition = Acquisition::new();
        let hr_msg = HeartrateMessage::default();
        acquisition.add_measurement(hr_msg);
        assert_eq!(acquisition.get_measurements().len(), 1);
    }
}
