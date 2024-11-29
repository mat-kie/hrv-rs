//! Bluetooth Controller
//!
//! This module defines the controller responsible for managing Bluetooth operations.
//! It interfaces with the Bluetooth model and handles events related to BLE devices.

use crate::core::events::{AppEvent, BluetoothEvent};
use crate::model::bluetooth::{AdapterHandle, BluetoothModelApi};

use btleplug::api::BDAddr;
use log::info;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Handles specific Bluetooth events.
///
/// # Arguments
/// - `controller`: Reference to the Bluetooth controller.
/// - `event`: The Bluetooth event to handle.
pub async fn handle_event<'a, AHT: AdapterHandle, CT: BluetoothApi<AHT>>(
    controller: &mut CT,
    event: BluetoothEvent,
) -> Result<(), String> {
    match event {
        BluetoothEvent::DiscoverAdapters => controller.discover_adapters().await,
        BluetoothEvent::AdapterSelected(uuid) => {
            controller.get_model().lock().await.select_adapter(&uuid)?;
            controller.start_scan().await
        }
        BluetoothEvent::StartListening(id) => controller.start_listening(id).await,
        BluetoothEvent::StopListening => controller.stop_listening().await,
        _ => Err("Unhandled BluetoothEvent".into()),
    }
}

/// API for Bluetooth operations.
pub trait BluetoothApi<AHT: AdapterHandle>: Send + Sync {
    /// Initializes the controller with the event transmitter.
    fn initialize(&mut self, tx: Sender<AppEvent>);

    /// Returns a reference to the Bluetooth model.
    fn get_model(&self) -> &Mutex<dyn BluetoothModelApi<AHT>>;

    /// Discovers available Bluetooth adapters.
    fn discover_adapters<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Starts scanning for peripherals.
    fn start_scan<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Stops scanning for peripherals.
    #[allow(dead_code)]
    fn stop_scan<'a>(&'a mut self)
        -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Starts listening for notifications from a specific peripheral.
    fn start_listening<'a>(
        &'a mut self,
        peripheral_addr: BDAddr,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Stops listening for notifications from peripherals.
    fn stop_listening<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

/// The Bluetooth Controller manages BLE interactions, including scanning for devices
/// and starting listening sessions.
pub struct BluetoothController<AHT: AdapterHandle + 'static> {
    /// The Bluetooth model instance.
    model: Arc<Mutex<dyn BluetoothModelApi<AHT>>>,
    /// Handle for the peripheral updater task.
    peri_updater_handle: Option<JoinHandle<Result<(), String>>>,
    /// Handle for the listener task.
    listener_handle: Option<JoinHandle<Result<(), String>>>,
    /// Event transmitter.
    tx: Option<Sender<AppEvent>>,
}

impl<AHT: AdapterHandle> BluetoothController<AHT> {
    /// Creates a new `BluetoothController` instance.
    ///
    /// # Arguments
    /// - `model`: The Bluetooth model instance.
    pub fn new(model: Arc<Mutex<dyn BluetoothModelApi<AHT>>>) -> Self {
        info!("BluetoothController initialized.");
        Self {
            model,
            peri_updater_handle: None,
            listener_handle: None,
            tx: None,
        }
    }

    /// Launches a peripheral updater task.
    async fn launch_periphal_updater<AT: AdapterHandle + 'static>(
        &self,
        adapter: AT,
    ) -> Result<JoinHandle<Result<(), String>>, String> {
        let model = self.model.clone();

        Ok(tokio::spawn(async move {
            loop {
                let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
                let mut device_list = Vec::new();
                for peripheral in peripherals {
                    device_list.push((peripheral.address, peripheral.name));
                }
                device_list.sort_by(|a, b| a.0.cmp(&b.0));
                model.lock().await.set_devices(device_list);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }))
    }
}

impl<AHT: AdapterHandle + Send + Sync> BluetoothApi<AHT> for BluetoothController<AHT> {
    fn initialize(&mut self, tx: Sender<AppEvent>) {
        self.tx = Some(tx);
    }

    fn get_model(&self) -> &Mutex<dyn BluetoothModelApi<AHT>> {
        &self.model
    }

    fn discover_adapters<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            self.model
                .lock()
                .await
                .set_adapters({ AHT::retrieve_adapters().await }?);
            Ok(())
        })
    }

    fn start_scan<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if self.model.lock().await.is_scanning() {
                return Err("Already scanning!".to_owned());
            }
            let adapter_handle = self.model.lock().await.get_selected_adapter().clone();
            if let Some(handle) = adapter_handle {
                handle.start_scan().await?;
                info!("Scanning started on adapter {}.", handle.name());
                if self.peri_updater_handle.is_none() {
                    self.peri_updater_handle = Some(self.launch_periphal_updater(handle).await?);
                }
                Ok(())
            } else {
                Err("No selected Bluetooth adapter!".to_owned())
            }
        })
    }

    fn stop_scan<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if !self.model.lock().await.is_scanning() {
                return Err("Not scanning!".to_owned());
            }
            let adapter_handle = self.model.lock().await.get_selected_adapter().clone();
            if let Some(handle) = adapter_handle {
                handle.stop_scan().await?;
                info!("Stopped scanning on adapter {}.", handle.name());
                if let Some(updater_handle) = &self.peri_updater_handle.take() {
                    updater_handle.abort();
                }
                Ok(())
            } else {
                Err("No selected Bluetooth adapter!".to_owned())
            }
        })
    }

    fn start_listening<'a>(
        &'a mut self,
        peripheral_addr: BDAddr,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let adapter = self.model.lock().await.get_selected_adapter().clone();
            if let Some(handle) = adapter {
                if let Some(tx) = self.tx.clone() {
                    self.listener_handle =
                        Some(handle.listen_to_peripheral(peripheral_addr, tx).await?);
                } else {
                    return Err("No event transmitter channel!".into());
                }
                self.model.lock().await.set_listening(Some(peripheral_addr));
                Ok(())
            } else {
                Err("No adapter selected for listening.".into())
            }
        })
    }

    fn stop_listening<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(handle) = &self.listener_handle {
                handle.abort();
                self.model.lock().await.set_listening(None);
                Ok(())
            } else {
                Err("No active listening task!".to_owned())
            }
        })
    }
}
