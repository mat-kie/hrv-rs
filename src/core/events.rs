//! Core Events
//!
//! This module defines events used for communication between different components
//! of the HRV analysis tool. Events are central to the application's event-driven architecture.

use std::path::PathBuf;
use time::Duration;

use crate::model::bluetooth::{AdapterDescriptor, DeviceDescriptor, HeartrateMessage};

/// Enumeration of Bluetooth-related events.
///
/// These events handle discovery, selection, and communication with BLE devices.
#[derive(Clone, Debug, PartialEq)]
pub enum BluetoothEvent {
    /// Discover all available Bluetooth adapters.
    #[allow(dead_code)]
    DiscoveredAdapters,
    /// An incoming heart rate message for processing.
    HrMessage(HeartrateMessage),
    /// Stop listening to Bluetooth devices.
    #[allow(dead_code)]
    StoppedListening,
    /// Stop scanning for Bluetooth devices.
    #[allow(dead_code)]
    StoppedScanning,
}

/// Enumeration of events related to HRV (Heart Rate Variability) data processing.
///
/// These events include changes to analysis parameters and incoming data messages.
#[derive(Clone, Debug, PartialEq)]
pub enum UiInputEvent {
    /// The time window for analysis has been changed.
    ///
    /// # Fields
    /// - `Duration`: The new time window duration.
    TimeWindowChanged(Duration),

    /// The outlier filter value has been updated.
    ///
    /// # Fields
    /// - `f64`: The new filter value.
    OutlierFilterChanged(f64),

    /// A stored acquisition has been selected.
    ///
    /// # Fields
    /// - `usize`: The index of the selected acquisition.
    StoredAcqSelected(usize),

    /// A request to start data acquisition.
    AcquisitionStartReq,
    /// A request to stop data acquisition.
    AcquisitionStopReq,
    /// A request to store the current acquisition.
    StoreAcquisition,
    /// A request to discard the current acquisition.
    DiscardAcquisition,
    /// Select a Bluetooth adapter.
    ///
    /// # Fields
    /// - `AdapterDescriptor`: The descriptor of the selected adapter.
    SelectAdapter(AdapterDescriptor),
    /// Select a Bluetooth peripheral.
    ///
    /// # Fields
    /// - `DeviceDescriptor`: The descriptor of the selected peripheral.
    SelectPeripheral(DeviceDescriptor),
    /// Prepare for a new acquisition.
    PrepareAcquisition,
    /// Load a model from a file.
    ///
    /// # Fields
    /// - `PathBuf`: The path to the model file.
    LoadModel(PathBuf),
    /// Store the current model to a file.
    ///
    /// # Fields
    /// - `PathBuf`: The path to the model file.
    StoreModel(PathBuf),
    /// Create a new model.
    NewModel,
}

/// Enumeration of all application-level events.
///
/// These events drive the interaction between views, controllers, and models.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Bluetooth-related events.
    Bluetooth(BluetoothEvent),
    /// HRV data-related events, such as updates or parameter changes.
    UiInput(UiInputEvent),
}
