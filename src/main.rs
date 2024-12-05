//! HRV Analysis Tool
//!
//! This tool processes data from Bluetooth Low Energy (BLE) chest straps to
//! analyze Heart Rate Variability (HRV). It integrates various modules for
//! data acquisition, BLE communication, and HRV computation. The tool is
//! structured using a modular, event-driven MVC architecture.

use controller::{
    acquisition::AcquisitionController, application::AppController, bluetooth::BluetoothController,
};
use eframe::NativeOptions;

use model::storage::StorageModel;
use model::{acquisition::AcquisitionModel, bluetooth::BluetoothModel};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::{broadcast, RwLock};

/// Core utilities and traits used throughout the application.
mod core {
    /// Application-wide constants.
    pub mod constants;
    /// Event system for inter-module communication.
    pub mod events;
    /// Trait definitions for views.
    pub mod view_trait;
}

/// Controllers managing the application's logic.
mod controller {
    /// Manages data acquisition from BLE devices.
    pub mod acquisition;
    /// Entry point controller for initializing and orchestrating modules.
    pub mod application;
    /// Handles communication with BLE devices.
    pub mod bluetooth;
}

/// Mathematical utilities for HRV analysis.
mod math {
    /// Functions and structures for HRV computation.
    pub mod hrv;
}

/// Data models representing the application's domain.
mod model {
    /// Model for acquisition of HRV and raw data.
    pub mod acquisition;
    /// Model for managing Bluetooth communication.
    pub mod bluetooth;
    /// Model for HRV-related data storage and processing.
    pub mod hrv;
    /// Model for general data storage.
    pub mod storage;
}

/// UI-related components for the application.
mod view {
    /// Bluetooth device management UI.
    pub mod acquisition;
    /// Manages transitions between views.
    pub mod manager;
    /// HRV analysis user interface.
    pub mod overview;
}

/// Main entry point of the application.
///
/// This function performs the following tasks:
/// 1. Initializes the logger for debugging and informational output.
/// 2. Sets up a Tokio runtime for handling asynchronous operations.
/// 3. Creates a broadcast channel for event-driven communication between modules.
/// 4. Initializes shared state models for Bluetooth and data storage.
/// 5. Starts the eframe application with the main view manager.
///
/// The application is structured using a modular, event-driven MVC architecture.
fn main() {
    // Initialize logger
    env_logger::init();

    // Create a new Tokio runtime for asynchronous operations.
    let rt = Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    // Create a broadcast channel for event-driven communication.
    let (event_bus, _) = broadcast::channel(16);

    // Shared state for Bluetooth model.
    let bluetooth_model = Arc::new(RwLock::new(BluetoothModel::default()));
    // Shared state for data storage model.
    let storage = Arc::new(RwLock::new(StorageModel::<AcquisitionModel>::default()));

    // Start the eframe application with the main view manager.
    eframe::run_native(
        "Hrv-rs",
        NativeOptions::default(),
        Box::new(|cc| {
            let app = AppController::new(
                BluetoothController::new(bluetooth_model, event_bus.clone()),
                AcquisitionController::new(storage.clone(), event_bus.clone()),
                storage,
                event_bus,
            );
            let res = Box::new(app.get_viewmanager());
            tokio::spawn(app.event_handler(cc.egui_ctx.clone()));
            Ok(res)
        }),
    )
    .expect("Failed to start eframe application");
}
