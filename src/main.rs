//! HRV Analysis Tool
//!
//! This tool processes data from Bluetooth Low Energy (BLE) chest straps to
//! analyze Heart Rate Variability (HRV). It integrates various modules for
//! data acquisition, BLE communication, and HRV computation. The tool is
//! structured using a modular, event-driven MVC architecture.

use btleplug::platform::Adapter;

use components::application::AppController;
use components::bluetooth::BluetoothComponent;
use components::measurement::MeasurementData;
use components::storage::StorageComponent;
use eframe::NativeOptions;

use tokio::runtime::Runtime;
use tokio::sync::broadcast;

/// Core utilities and traits used throughout the application.
mod core {
    /// Application-wide constants.
    pub mod constants;
    /// Event system for inter-module communication.
    pub mod events;
}

mod api {
    pub mod controller;
    pub mod model;
    pub mod view;
}
/// Controllers managing the application's logic.
mod components {
    /// Entry point controller for initializing and orchestrating modules.
    pub mod application;
    /// Handles communication with BLE devices.
    pub mod bluetooth;
    pub mod measurement;
    /// Manages data acquisition from BLE devices.
    pub mod storage;
}

/// Data models representing the application's domain.
mod model {

    /// Model for managing Bluetooth communication.
    pub mod bluetooth;
    /// Model for HRV-related data storage and processing.
    pub mod hrv;
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
    let bluetooth = BluetoothComponent::<Adapter>::new(event_bus.clone());
    // Shared state for data storage model.
    let storage = StorageComponent::<MeasurementData>::default();

    let app = AppController::new(bluetooth, storage, event_bus.clone());
    // Start the eframe application with the main view manager.
    eframe::run_native(
        "Hrv-rs",
        NativeOptions::default(),
        Box::new(|cc| {
            let res = Box::new(app.get_viewmanager());
            tokio::spawn(app.event_handler(cc.egui_ctx.clone()));
            Ok(res)
        }),
    )
    .expect("Failed to start eframe application");
}
