//! Controller Module
//!
//! This module defines the traits and structures for managing the application's core functionalities,
//! including recording, storage, and Bluetooth interactions. It provides a set of asynchronous APIs
//! to handle various operations such as starting/stopping recordings, loading/storing data, and managing
//! Bluetooth devices.
use crate::model::bluetooth::{AdapterDescriptor, DeviceDescriptor, HeartrateMessage};
use anyhow::Result;
use async_trait::async_trait;
use btleplug::api::Central;
use std::{path::PathBuf, sync::Arc};
use time::Duration;
use tokio::sync::RwLock;

use super::model::{BluetoothModelApi, MeasurementModelApi};

#[derive(Clone, Debug)]
pub enum OutlierFilter {
    MovingMAD { parameter: f64, _window: usize },
}

/// RecordingApi trait
///
/// This trait defines the asynchronous API for managing the recording process in the application.
/// It provides methods to start and stop the recording process.
#[async_trait]
pub trait RecordingApi {
    /// start the recording process
    async fn start_recording(&mut self) -> Result<()>;
    /// stop the recording process
    async fn stop_recording(&mut self) -> Result<()>;
}

/// StorageEventApi trait
///
/// This trait defines the asynchronous API for managing storage-related events in the application.
/// It provides methods to clear storage, load data from a file, store data to a file, and handle
/// new and recorded measurements.
#[async_trait]
pub trait StorageEventApi {
    /// Clear the storage.
    ///
    /// This method clears all the stored data.
    async fn clear(&mut self) -> Result<()>;

    /// Load data from a file.
    ///
    /// This method loads data from the specified file path.
    ///
    /// # Arguments
    ///
    /// * `path` - A `PathBuf` representing the file path from which to load data.
    async fn load_from_file(&mut self, path: PathBuf) -> Result<()>;

    /// Store data to a file.
    ///
    /// This method stores data to the specified file path.
    ///
    /// # Arguments
    ///
    /// * `path` - A `PathBuf` representing the file path to which to store data.
    async fn store_to_file(&mut self, path: PathBuf) -> Result<()>;

    /// Store the recorded measurement.
    ///
    /// This method handles the storage of a new measurement.
    async fn new_measurement(&mut self) -> Result<()>;

    /// Store the recorded measurement.
    ///
    /// This method handles the storage of the recorded measurement.
    async fn store_recorded_measurement(&mut self) -> Result<()>;
}

/// StorageApi trait
///
/// This trait defines the asynchronous API for managing storage operations in the application.
/// It provides methods to interact with the active measurement.
///
/// # Type Parameters
///
/// * `MT` - A type that implements the `MeasurementModelApi` trait.
///
pub trait StorageApi<MT: MeasurementModelApi> {
    /// Get the active measurement.
    ///
    /// This method returns a reference to the active measurement, if any.
    fn get_active_measurement(&mut self) -> &Option<Arc<RwLock<MT>>>;
}

/// MeasurementApi trait
///
/// This trait extends the `MeasurementModelApi` trait and defines additional asynchronous APIs
/// for mutatung measurement-related operations in the application. It provides methods to set
/// statistical windows, configure outlier filters, and record heart rate messages.
#[async_trait]
pub trait MeasurementApi: MeasurementModelApi {
    /// Set the statistics window.
    ///
    /// This method sets the duration of the window used for statistical calculations.
    ///
    /// # Arguments
    ///
    /// * `window` - A `Duration` representing the length of the statistics window.
    async fn set_stats_window(&mut self, window: Duration) -> Result<()>;

    /// Set the outlier filter.
    ///
    /// This method configures the outlier filter used to process the measurements.
    ///
    /// # Arguments
    ///
    /// * `filter` - An `OutlierFilter` specifying the type and parameters of the filter.
    async fn set_outlier_filter(&mut self, filter: OutlierFilter) -> Result<()>;

    /// Record a heart rate message.
    ///
    /// This method processes and records a new heart rate message.
    ///
    /// # Arguments
    ///
    /// * `msg` - A `HeartrateMessage` containing the heart rate data to be recorded.
    async fn record_message(&mut self, msg: HeartrateMessage) -> Result<()>;
}

/// BluetoothApi trait
///
/// This trait extends the `BluetoothModelApi` trait and defines additional asynchronous APIs
/// for mutating Bluetooth operations in the application. It provides methods to discover adapters,
/// select adapters and peripherals, start and stop scanning, and start and stop listening for
/// Bluetooth events.
#[async_trait]
pub trait BluetoothApi: BluetoothModelApi + Send + Sync {
    /// Discover Bluetooth adapters.
    ///
    /// This method initiates the discovery of available Bluetooth adapters.
    async fn discover_adapters(&mut self) -> Result<()>;

    /// Select a Bluetooth adapter.
    ///
    /// This method selects a Bluetooth adapter based on the provided adapter descriptor.
    ///
    /// # Arguments
    ///
    /// * `adapter` - An `AdapterDescriptor` representing the unique identifier of the adapter to be selected.
    async fn select_adapter(&mut self, adapter: AdapterDescriptor) -> Result<()>;

    /// Select a Bluetooth peripheral.
    ///
    /// This method selects a Bluetooth peripheral based on the provided device descriptor.
    ///
    /// # Arguments
    ///
    /// * `device` - A `DeviceDescriptor` representing the unique identifier of the peripheral to be selected.
    async fn select_peripheral(&mut self, device: DeviceDescriptor) -> Result<()>;

    /// Start scanning for Bluetooth devices.
    ///
    /// This method initiates the scanning process to discover Bluetooth peripherals.
    async fn start_scan(&mut self) -> Result<()>;

    /// Stop scanning for Bluetooth devices.
    ///
    /// This method stops the ongoing scanning process for discovering Bluetooth peripherals.
    #[allow(dead_code)]
    async fn stop_scan(&mut self) -> Result<()>;

    /// Start listening to the last selected bluetooth peripheral
    async fn start_listening(&mut self) -> Result<()>;

    /// Stop listening to the bluetooth peripheral
    async fn stop_listening(&mut self) -> Result<()>;
}

/// AdapterDiscovery trait
///
/// This trait defines the asynchronous API for discovering Bluetooth adapters in the application.
/// It provides a method to discover available Bluetooth adapters.
///
/// # Type Parameters
///
/// * `A` - A type that implements the `Central` and `DisplayName` traits.
///
#[async_trait]
pub trait AdapterDiscovery<A: Central + DisplayName> {
    /// Discover Bluetooth adapters.
    ///
    /// This method initiates the discovery of available Bluetooth adapters and returns a vector of adapters.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of discovered adapters of type `A` on success, or an error on failure.
    async fn discover_adapters() -> Result<Vec<A>>;
}

/// DisplayName trait
///
/// This trait defines the asynchronous API for retrieving the display name of an object.
/// It provides a method to get the name to display for the implementing object.
#[async_trait]
pub trait DisplayName {
    /// Get the name to display for the implementing object.
    ///
    /// This method returns the display name of the object as a `String`.
    ///
    /// # Returns
    ///
    /// A `Result` containing the display name of the object as a `String` on success, or an error on failure.
    async fn get_name(&self) -> Result<String>;
}
