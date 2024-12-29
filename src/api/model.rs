//! This module defines the read only API for interacting with various models.
//! It provides interfaces for accessing data related to HRV measurements,
//! Bluetooth adapters, and stored acquisitions.
use btleplug::api::BDAddr;
use std::{fmt::Debug, sync::Arc};
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;

use crate::model::{
    bluetooth::{AdapterDescriptor, DeviceDescriptor, HeartrateMessage},
    hrv::HrvAnalysisData,
};

/// `MeasurementModelApi` trait.
///
/// Defines the interface for managing measurement-related data, including runtime measurements,
/// HRV statistics, and stored acquisitions.
pub trait MeasurementModelApi: Debug + Send + Sync {
    /// Retrieves the start time of the current acquisition.
    ///
    /// # Returns
    /// An `OffsetDateTime` indicating the start time.
    fn get_start_time(&self) -> &OffsetDateTime;

    /// Retrieves the last heart rate message received.
    ///
    /// # Returns
    /// An optional `HeartrateMessage` representing the most recent measurement.
    fn get_last_msg(&self) -> Option<&HeartrateMessage>;

    fn get_rmssd(&self) -> Option<f64>;
    fn get_sdrr(&self) -> Option<f64>;
    fn get_sd1(&self) -> Option<f64>;
    fn get_sd2(&self) -> Option<f64>;
    fn get_hr(&self) -> Option<f64>;
    fn get_dfa1a(&self) -> Option<f64>;

    fn get_rmssd_ts(&self) -> Vec<[f64; 2]>;
    fn get_sdrr_ts(&self) -> Vec<[f64; 2]>;
    fn get_sd1_ts(&self) -> Vec<[f64; 2]>;
    fn get_sd2_ts(&self) -> Vec<[f64; 2]>;
    fn get_hr_ts(&self) -> Vec<[f64; 2]>;
    fn get_dfa1a_ts(&self) -> Vec<[f64; 2]>;

    /// Retrieves the configured statistics window.
    ///
    /// # Returns
    /// A reference to an optional `Duration` representing the analysis window size.
    fn get_stats_window(&self) -> Option<usize>;

    /// Getter for the filter parameter value (fraction of std. dev).
    ///
    /// # Returns
    /// The parameter value for the outlier filter.
    fn get_outlier_filter_value(&self) -> f64;

    /// Retrieves the points for the Poincare plot.
    ///
    /// # Returns
    /// A vector of `[f64; 2]` pairs representing the Poincare points.
    fn get_poincare_points(&self) -> (Vec<[f64; 2]>, Vec<[f64; 2]>);

    /// Retrieves the session data.
    ///
    /// # Returns
    /// A reference to the `HrvSessionData`.
    fn get_session_data(&self) -> &HrvAnalysisData;

    /// Retrieves the elapsed time since the start of the acquisition.
    ///
    /// # Returns
    /// A `Duration` representing the elapsed time.
    fn get_elapsed_time(&self) -> Duration;
}

pub trait BluetoothModelApi: Debug + Send + Sync {
    /// Gets the list of Bluetooth adapters as a vector of `(Name, UUID)` tuples.
    ///
    /// # Returns
    /// A vector of tuples containing adapter names and UUIDs.
    fn get_adapters(&self) -> &[AdapterDescriptor];

    /// Gets the currently selected adapter, if any.
    ///
    /// # Returns
    /// An optional reference to the selected adapter.
    fn get_selected_adapter(&self) -> Option<AdapterDescriptor>;

    /// Gets the list of discovered Bluetooth devices.
    ///
    /// # Returns
    /// A reference to the vector of devices.
    fn get_devices(&self) -> &Arc<RwLock<Vec<DeviceDescriptor>>>;

    fn get_selected_device(&self) -> Option<DeviceDescriptor>;

    /// Gets the scanning status.
    ///
    /// # Returns
    /// `true` if scanning is active, `false` otherwise.
    #[allow(dead_code)]
    fn is_scanning(&self) -> bool;

    fn is_listening_to(&self) -> Option<BDAddr>;
}

pub trait StorageModelApi: Debug + Sync + Send {
    /// Returns a slice of handles to the stored acquisition models.
    fn get_acquisitions(&self) -> &[ModelHandle<dyn MeasurementModelApi>];
}

pub type ModelHandle<T> = Arc<RwLock<T>>;
