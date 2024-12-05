//! Bluetooth Controller
//!
//! This module defines the controller responsible for managing Bluetooth operations.
//! It interfaces with the Bluetooth model and handles events related to BLE devices.

use crate::core::constants::HEARTRATE_MEASUREMENT_UUID;
use crate::core::events::BluetoothEvent;
use crate::map_err;
use crate::model::bluetooth::{BluetoothModelApi, DeviceDescriptor, HeartrateMessage};
use crate::model::storage::ModelHandle;
use crate::{core::events::AppEvent, model::bluetooth::AdapterDescriptor};
use btleplug::api::{Peripheral, ScanFilter};
use btleplug::{
    api::{BDAddr, Central, Manager as _},
    platform::{Adapter, Manager},
};

use egui::ahash::HashMap;
use futures::StreamExt;
use log::{info, warn};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// API for Bluetooth operations.
pub trait BluetoothApi: Send + Sync {
    /// Returns a reference to the Bluetooth model.
    fn get_model(&self) -> ModelHandle<dyn BluetoothModelApi>;

    /// Discovers available Bluetooth adapters.
    fn discover_adapters<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Discovers available Bluetooth adapters.
    fn select_adapter<'a>(
        &'a self,
        uuid: &'a AdapterDescriptor,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn select_peripheral<'a>(
        &'a self,
        uuid: &'a DeviceDescriptor,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Starts scanning for peripherals.
    fn start_scan<'a>(
        &'a mut self
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Stops scanning for peripherals.
    #[allow(dead_code)]
    fn stop_scan<'a>(&'a mut self)
        -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Starts listening for notifications from a specific peripheral.
    fn start_listening<'a>(
        &'a mut self
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Stops listening for notifications from peripherals.
    fn stop_listening<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

/// The Bluetooth Controller manages BLE interactions, including scanning for devices
/// and starting listening sessions.
pub struct BluetoothController {
    /// The Bluetooth model instance.
    model: Arc<RwLock<dyn BluetoothModelApi>>,
    event_bus: Sender<AppEvent>,
    /// Handle for the peripheral updater task.
    peri_updater_handle: Option<JoinHandle<Result<(), String>>>,
    /// Handle for the listener task.
    listener_handle: Option<JoinHandle<Result<(), String>>>,
    /// Event transmitter.
    adapters: HashMap<Uuid, Adapter>,
}

impl BluetoothController {
    /// Creates a new `BluetoothController` instance.
    ///
    /// # Arguments
    /// - `model`: The Bluetooth model instance.
    pub fn new(model: Arc<RwLock<dyn BluetoothModelApi>>, event_bus: Sender<AppEvent>) -> Self {
        info!("BluetoothController initialized.");
        Self {
            model,
            event_bus,
            peri_updater_handle: None,
            listener_handle: None,
            adapters: Default::default(),
        }
    }

    async fn get_adapter(&self) -> Option<&Adapter> {
        if let Some(desc) = self.model.read().await.get_selected_adapter() {
            self.adapters.get(desc.get_uuid())
        } else {
            None
        }
    }

    fn listen_to_peripheral<'a>(
        adapter: Adapter,
        peripheral_address: BDAddr,
        tx: Sender<AppEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<JoinHandle<Result<(), String>>, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let peripherals = map_err!(adapter.peripherals().await)?;
            let cheststrap = peripherals
                .into_iter()
                .find(|p| p.address() == peripheral_address)
                .ok_or("Peripheral not found")?;
            map_err!(cheststrap.connect().await)?;

            cheststrap
                .discover_services()
                .await
                .map_err(|e| e.to_string())?;

            if let Some(char) = cheststrap
                .characteristics()
                .iter()
                .find(|c| c.uuid == HEARTRATE_MEASUREMENT_UUID)
            {
                cheststrap
                    .subscribe(char)
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                return Err(format!(
                    "Peripheral has no Heartrate characteristic! {}",
                    map_err!(cheststrap.properties().await)?
                        .unwrap_or_default()
                        .local_name
                        .unwrap_or("Unknown".to_owned())
                ));
            }

            let mut notification_stream = cheststrap
                .notifications()
                .await
                .map_err(|e| e.to_string())?;
            let fut = tokio::spawn(async move {
                while let Some(data) = notification_stream.next().await {
                    if data.value.len() < 2
                        || tx
                            .send(AppEvent::Bluetooth(BluetoothEvent::HrMessage(HeartrateMessage::new(
                                &data.value,
                            ))))
                            .is_err()
                    {
                        break;
                    }
                }
                warn!("BT transciever terminated");
                Err("listener terminated".into())
            });
            Ok(fut)
        })
    }
    // Launches a peripheral updater task.
    async fn launch_periphal_updater(
        &self,
        adapter: Adapter,
        _channel: Sender<AppEvent>,
    ) -> Result<JoinHandle<Result<(), String>>, String> {
        let model = self.model.clone();

        Ok(tokio::spawn(async move {
            loop {
                let peripherals = adapter.peripherals().await.map_err(|e| e.to_string())?;
                let mut descriptors = Vec::new();
                for peripheral in &peripherals {
                    let address = peripheral.address();
                    if let Some(props) = map_err!(peripheral.properties().await)? {
                        if let Some(name) = props.local_name {
                            descriptors.push(DeviceDescriptor { name, address });
                        }
                    }
                }
                // TODO: Send events when an error arises
                descriptors.sort();
                model.write().await.set_devices(descriptors);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }))
    }
}

impl BluetoothApi for BluetoothController {
    fn select_peripheral<'a>(
        &'a self,
        uuid: &'a DeviceDescriptor,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            self.model.write().await.select_device(uuid.clone());
            Ok(())
        })
    }
    fn select_adapter<'a>(
        &'a self,
        adapter: &'a AdapterDescriptor,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { self.model.write().await.select_adapter(adapter.get_uuid()) })
    }

    fn get_model(&self) -> ModelHandle<dyn BluetoothModelApi> {
        self.model.clone().into()
    }

    fn discover_adapters<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let manager = Manager::new().await.map_err(|e| e.to_string())?;
            let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
            let mut model = Vec::new();
            for adapter in adapters {
                let name = adapter.adapter_info().await.unwrap_or("unknown".into());
                let desc = AdapterDescriptor::new(name);
                model.push(desc.clone());
                self.adapters.insert(*desc.get_uuid(), adapter);
            }
            self.model.write().await.set_adapters(model);

            Ok(())
        })
    }

    fn start_scan<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if self.model.read().await.is_scanning() {
                return Err("Already scanning!".to_owned());
            }
            let adapter_handle = self.get_adapter().await;
            if let Some(handle) = adapter_handle {
                handle.start_scan(ScanFilter::default()).await;
                info!(
                    "Scanning started on adapter {}.",
                    handle.adapter_info().await.unwrap_or("unknown".into())
                );
                if self.peri_updater_handle.is_none() {
                    self.peri_updater_handle = Some(
                        self.launch_periphal_updater(handle.clone(), self.event_bus.clone())
                            .await?,
                    );
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
            if !self.model.read().await.is_scanning() {
                return Err("Not scanning!".to_owned());
            }
            let adapter_handle = self.get_adapter().await;
            if let Some(handle) = adapter_handle {
                handle.stop_scan().await;
                info!(
                    "Stopped scanning on adapter {}.",
                    handle.adapter_info().await.unwrap_or("unknown".into())
                );
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
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let adapter = self.get_adapter().await;
            if let Some(jh) = &self.listener_handle {
                jh.abort();
            }

            if let Some(handle) = adapter {
                let mut model = self.model.write().await;
                let dev = model.get_selected_device().clone();
                if let Some(desc) = dev {
                    self.listener_handle = Self::listen_to_peripheral(handle.clone(), desc.address, self.event_bus.clone())
                        .await
                        .ok();
                    model.set_listening(Some(desc.address));
                    Ok(())
                } else {
                    Err("No devive selected for listening.".into())
                }
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
                self.model.write().await.set_listening(None);
                Ok(())
            } else {
                Err("No active listening task!".to_owned())
            }
        })
    }
}
