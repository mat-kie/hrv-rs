//! HRV Analysis Tool
//!
//! This tool processes data from Bluetooth Low Energy (BLE) chest straps to
//! analyze Heart Rate Variability (HRV). It integrates various modules for
//! data acquisition, BLE communication, and HRV computation.

use controller::{
    acquisition::AcquisitionController, application::AppController, bluetooth::BluetoothController,
};
use eframe::NativeOptions;
use env_logger::Env;
#[cfg(not(feature="mock"))]
use model::bluetooth::BluetoothAdapter;
#[cfg(feature="mock")]
use model::bluetooth::MockAdapterHandle;

use model::{acquisition::AcquisitionModel, bluetooth::BluetoothModel};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;

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
}

/// UI-related components for the application.
mod view {
    /// Bluetooth device management UI.
    pub mod bluetooth;
    /// HRV analysis user interface.
    pub mod hrv_analysis;
    /// View manager for coordinating multiple views.
    pub mod manager;
    /// View for model initialization
    pub mod model_initializer;
}

/// Main entry point of the application.
///
/// Initializes logging, sets up asynchronous runtime, and starts the
/// application with the eframe framework.
fn main() {
    // Initialize logger with environment-specific settings.
    env_logger::Builder::from_env(
        Env::default()
            .filter_or("MY_LOG_LEVEL", "info")
            .write_style_or("MY_LOG_STYLE", "always"),
    )
    .init();

    // Create a new Tokio runtime for asynchronous operations.
    let rt = Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    // Shared state for Bluetooth model.
    #[cfg(feature="mock")]
    let bluetooth_model = Arc::new(Mutex::new(BluetoothModel::<MockAdapterHandle>::default()));
    #[cfg(not(feature="mock"))]
    let bluetooth_model = Arc::new(Mutex::new(BluetoothModel::<BluetoothAdapter>::default()));

    // Shared state for acquisition model.
    let acquisition_model = Arc::new(std::sync::Mutex::new(AcquisitionModel::default()));

    // Initialize application controller with models and controllers.
    let app_controller = AppController::new(
        bluetooth_model.clone(),
        acquisition_model.clone(),
        BluetoothController::new(bluetooth_model.clone()),
        AcquisitionController::new(acquisition_model.clone()),
    );

    // Start the eframe application with the main view manager.
    eframe::run_native(
        "Hrv-rs",
        NativeOptions::default(),
        Box::new(|cc| {
            let view_manager = app_controller.launch(cc.egui_ctx.clone());
            Ok(Box::new(view_manager))
        }),
    )
    .expect("Failed to start eframe application");
}
