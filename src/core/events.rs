//! Core Events
//!
//! This module defines events used for communication between different components
//! of the HRV analysis tool. Events are central to the application's event-driven architecture.
use btleplug::api::BDAddr;
use std::{path::PathBuf, sync::Arc};
use time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::model::{acquisition::AcquisitionModelApi, bluetooth::HeartrateMessage};

/// Enumeration of Bluetooth-related events.
///
/// These events handle discovery, selection, and communication with BLE devices.
#[derive(Clone, Debug, PartialEq)]
pub enum BluetoothEvent {
    /// Discover all available Bluetooth adapters.
    DiscoverAdapters,
    /// Select a specific Bluetooth adapter by its UUID.
    AdapterSelected(Uuid),
    /// Start listening to a specific Bluetooth device.
    StartListening(BDAddr),
    /// Stop listening to Bluetooth devices.
    #[allow(dead_code)]
    StopListening,
    /// Stop scanning for Bluetooth devices.
    #[allow(dead_code)]
    StopScanning,
}

/// Enumeration of events related to HRV (Heart Rate Variability) data processing.
///
/// These events include changes to analysis parameters and incoming data messages.
#[derive(Clone, Debug, PartialEq)]
pub enum HrvEvent {
    /// The time window for analysis has been changed.
    ///
    /// # Fields
    /// - `Duration`: The new time window duration.
    #[allow(dead_code)]
    TimeWindowChanged(Duration),

    /// The outlier filter value has been updated.
    ///
    /// # Fields
    /// - `f64`: The new filter value.
    #[allow(dead_code)]
    OutlierFilterChanged(f64),

    /// An incoming heart rate message for processing.
    HrMessage(HeartrateMessage),
    /// A request to start data acquisition.
    AcquisitionStartReq,
    /// A request to stop data acquisition.
    AcquisitionStopReq,
}

/// Enumeration of all application-level events.
///
/// These events drive the interaction between views, controllers, and models.
#[derive(Debug)]
pub enum AppEvent {
    /// Bluetooth-related events.
    Bluetooth(BluetoothEvent),
    /// HRV data-related events, such as updates or parameter changes.
    Data(HrvEvent),
    NewAcquisition,
    DiscardAcquisition,
    StoreAcquisition,
    NewModel,
    LoadModel(PathBuf),
    StoreModel(PathBuf),
}
