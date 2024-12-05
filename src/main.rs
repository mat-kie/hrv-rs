//! HRV Analysis Tool
//!
//! This tool processes data from Bluetooth Low Energy (BLE) chest straps to
//! analyze Heart Rate Variability (HRV). It integrates various modules for
//! data acquisition, BLE communication, and HRV computation.

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
    /// Custom macros for code simplification.
    pub mod macros;
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
    pub mod storage;
}

/// UI-related components for the application.
mod view {
    /// Bluetooth device management UI.
    /// HRV analysis user interface.
    pub mod acquisition;

    pub mod overview;

    pub mod manager;
}

/// Main entry point of the application.
///
/// Initializes logging, sets up asynchronous runtime, and starts the
/// application with the eframe framework.
fn main() {
    // Initialize logger
    env_logger::init();

    // Create a new Tokio runtime for asynchronous operations.
    let rt = Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    let (event_bus, _) = broadcast::channel(16);

    // Shared state for Bluetooth model.
    let bluetooth_model = Arc::new(RwLock::new(BluetoothModel::default()));
    let storage = Arc::new(RwLock::new(StorageModel::<AcquisitionModel>::default()));
    // Shared state for acquisition model.

    // Initialize application controller with models and controllers.

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
