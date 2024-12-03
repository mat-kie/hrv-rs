//! Acquisition Model
//!
//! This module defines the model for managing acquisition data in the HRV analysis tool.
//! It provides structures and traits for handling real-time and stored data related to HRV.

use super::bluetooth::HeartrateMessage;
use crate::model::hrv::{HrvSessionData, HrvStatistics};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt::Debug, fs, path::PathBuf, sync::{Arc, Mutex}};
use time::{Duration, OffsetDateTime};

/// `AcquisitionModelApi` trait.
///
/// Defines the interface for managing acquisition-related data, including runtime measurements,
/// HRV statistics, and stored acquisitions.
#[typetag::serde(tag="type")]
pub trait AcquisitionModelApi: Debug + Send + Sync{

    fn reset(&mut self);
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

    fn get_session_data(&self)->&HrvSessionData;

    fn get_messages(&self)->&[(Duration, HeartrateMessage)];

}

/// Represents the acquisition model, managing HRV-related data and operations.
#[derive(Serialize, Debug, Clone)]
pub struct AcquisitionModel {
    /// The start time of the acquisition.
    start_time: OffsetDateTime,
    /// Collected measurements with their elapsed time.
    measurements: Vec<(Duration, HeartrateMessage)>,
    /// Window duration for statistical calculations.
    window: Option<Duration>,
    /// Outlier filter threshold.
    outlier_filter: f64,
    /// Processed session data.
    #[serde(skip)]
    sessiondata: HrvSessionData,
}

impl Default for AcquisitionModel {
    fn default() -> Self {
        Self {
            start_time: OffsetDateTime::now_utc(),
            measurements: Vec::new(),
            window: None,
            outlier_filter: 1.0,
            sessiondata: Default::default(),
        }
    }
}

impl<'de> Deserialize<'de> for AcquisitionModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct AcquisitionModelHelper {
            start_time: OffsetDateTime,
            measurements: Vec<(Duration, HeartrateMessage)>,
            window: Option<Duration>,
            outlier_filter: f64,
        }
        // Deserialize all fields except `sessiondata`
        let helper = AcquisitionModelHelper::deserialize(deserializer)?;

        // Reconstruct `sessiondata` from the `measurements`
        let sessiondata = HrvSessionData::from_acquisition(&helper.measurements, helper.window, helper.outlier_filter);

        Ok(AcquisitionModel {
            start_time: helper.start_time,
            measurements: helper.measurements,
            window: helper.window,
            outlier_filter: helper.outlier_filter,
            sessiondata,
        })
    }
}

impl AcquisitionModel {
    /// Updates the session data based on the current measurements.
    fn update(&mut self) {
        self.sessiondata = HrvSessionData::from_acquisition(&self.measurements, self.window, self.outlier_filter);
    }

    /// Adds a new heart rate measurement.
    pub fn add_measurement(&mut self, measurement: HeartrateMessage) {
        let elapsed = OffsetDateTime::now_utc() - self.start_time;
        self.measurements.push((elapsed, measurement));
        self.update();
    }

    /// Retrieves all measurements.
    pub fn get_measurements(&self) -> &[(Duration, HeartrateMessage)] {
        &self.measurements
    }

    /// Stores the model to a file.
    pub fn store(&self, path: PathBuf) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }

    /// Loads the model from a file.
    pub fn from_file(path: PathBuf) -> Result<Arc<Mutex<Self>>, String> {
        let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let model: Self = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(Arc::new(Mutex::new(model)))
    }
}

#[typetag::serde]
impl AcquisitionModelApi for AcquisitionModel {

    fn reset(&mut self){
        self.measurements.clear();
        self.start_time = OffsetDateTime::now_utc();
    }

    fn get_messages(&self)->&[(Duration, HeartrateMessage)] {
        &self.measurements
    }
    fn get_session_data(&self)->&HrvSessionData {
        &self.sessiondata
    }

    fn get_hrv_stats(&self) -> &Option<HrvStatistics> {
        &self.sessiondata.hrv_stats
    }

    fn get_poincare_points(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_poincare()
    }

    fn get_start_time(&self) -> Option<OffsetDateTime> {
        Some(self.start_time)
    }

    fn get_last_msg(&self) -> Option<HeartrateMessage> {
        self.measurements.last().map(|entry| entry.1)
    }

    fn get_stats_window(&self) -> &Option<Duration> {
        &self.window
    }

    fn add_measurement(&mut self, msg: &HeartrateMessage) {
        self.add_measurement(*msg);
    }

    fn set_stats_window(&mut self, window: &Duration) {
        self.window = Some(*window);
        self.update();
    }

    fn get_outlier_filter_value(&self) -> f64 {
        self.outlier_filter
    }

    fn set_outlier_filter_value(&mut self, value: f64) {
        if value >= 0.0 {
            self.outlier_filter = value;
        }
        self.update();
    }
}

