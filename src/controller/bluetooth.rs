//! Bluetooth Controller
//!
//! This module defines the controller responsible for managing Bluetooth operations.
//! It interfaces with the Bluetooth model and handles events related to BLE devices.

use crate::core::constants::HEARTRATE_MEASUREMENT_UUID;
use crate::core::events::BluetoothEvent;
use crate::model::bluetooth::{BluetoothModelApi, DeviceDescriptor, HeartrateMessage};
use crate::model::storage::ModelHandle;
use crate::{core::events::AppEvent, model::bluetooth::AdapterDescriptor};
use async_trait::async_trait;
use btleplug::api::{Peripheral, ScanFilter};
use btleplug::{
    api::{BDAddr, Central, Manager as _},
    platform::{Adapter, Manager},
};

use anyhow::{anyhow, Result};
use egui::ahash::HashMap;
use futures::StreamExt;
use log::{trace, warn};
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// API for Bluetooth operations.
#[async_trait]
pub trait BluetoothApi: Send + Sync {
    /// Returns a reference to the Bluetooth model.
    fn get_model(&self) -> Result<ModelHandle<dyn BluetoothModelApi>>;

    /// Discovers available Bluetooth adapters.
    async fn discover_adapters(&mut self) -> Result<()>;

    /// Selects a Bluetooth adapter.
    async fn select_adapter(&self, uuid: &AdapterDescriptor) -> Result<()>;

    /// Selects a Bluetooth peripheral.
    async fn select_peripheral(&self, uuid: &DeviceDescriptor) -> Result<()>;

    /// Starts scanning for peripherals.
    async fn start_scan(&mut self) -> Result<()>;

    /// Stops scanning for peripherals.
    async fn stop_scan(&mut self) -> Result<()>;

    /// Starts listening for notifications from a specific peripheral.
    async fn start_listening(&mut self) -> Result<()>;

    /// Stops listening for notifications from peripherals.
    async fn stop_listening(&mut self) -> Result<()>;
}

/// The Bluetooth Controller manages BLE interactions, including scanning for devices
/// and starting listening sessions.
pub struct BluetoothController {
    /// The Bluetooth model instance.
    model: Arc<RwLock<dyn BluetoothModelApi>>,
    event_bus: Sender<AppEvent>,
    /// Handle for the peripheral updater task.
    peri_updater_handle: Option<JoinHandle<Result<()>>>,
    /// Handle for the listener task.
    listener_handle: Option<JoinHandle<Result<()>>>,
    /// Event transmitter.
    adapters: HashMap<Uuid, Adapter>,
}

impl BluetoothController {
    /// Creates a new `BluetoothController` instance.
    ///
    /// # Arguments
    /// - `model`: The Bluetooth model instance.
    /// - `event_bus`: The event bus for broadcasting application events.
    ///
    /// # Returns
    /// A new `BluetoothController` instance.
    pub fn new(model: Arc<RwLock<dyn BluetoothModelApi>>, event_bus: Sender<AppEvent>) -> Self {
        Self {
            model,
            event_bus,
            peri_updater_handle: None,
            listener_handle: None,
            adapters: Default::default(),
        }
    }

    /// Retrieves the selected Bluetooth adapter.
    ///
    /// # Returns
    /// A reference to the selected adapter, or an error if none is selected.
    async fn get_adapter(&self) -> Result<&Adapter> {
        let model = self.model.read().await;
        let desc = model
            .get_selected_adapter()
            .as_ref()
            .ok_or(anyhow!("no selected adapter!"))?;
        self.adapters
            .get(desc.get_uuid())
            .ok_or(anyhow!("could not find the selected adapter"))
    }

    /// Listens to notifications from a specific peripheral.
    ///
    /// # Arguments
    /// - `adapter`: The Bluetooth adapter.
    /// - `peripheral_address`: The address of the peripheral.
    /// - `tx`: The event transmitter.
    ///
    /// # Returns
    /// A future that resolves to a join handle for the listener task.
    async fn listen_to_peripheral(
        adapter: Adapter,
        peripheral_address: BDAddr,
        tx: Sender<AppEvent>,
    ) -> Result<JoinHandle<Result<()>>> {
        let peripherals = adapter.peripherals().await?;
        let cheststrap = peripherals
            .into_iter()
            .find(|p| p.address() == peripheral_address)
            .ok_or(anyhow!("Peripheral not found"))?;
        cheststrap.connect().await?;

        cheststrap.discover_services().await?;

        let char = cheststrap
            .characteristics()
            .iter()
            .find(|c| c.uuid == HEARTRATE_MEASUREMENT_UUID)
            .ok_or(anyhow!("Peripheral has no Heartrate attribute"))?
            .clone();

        cheststrap.subscribe(&char).await?;

        let mut notification_stream = cheststrap.notifications().await?;
        let fut = tokio::spawn(async move {
            while let Some(data) = notification_stream.next().await {
                if data.value.len() < 2
                    || tx
                        .send(AppEvent::Bluetooth(BluetoothEvent::HrMessage(
                            HeartrateMessage::new(&data.value),
                        )))
                        .is_err()
                {
                    break;
                }
            }
            warn!("BT transceiver terminated");
            Err(anyhow!("listener terminated"))
        });
        Ok(fut)
    }

    /// Launches a peripheral updater task.
    ///
    /// # Arguments
    /// - `adapter`: The Bluetooth adapter.
    /// - `_channel`: The event transmitter.
    ///
    /// # Returns
    /// A future that resolves to a join handle for the updater task.
    async fn launch_periphal_updater(
        &self,
        adapter: Adapter,
        _channel: Sender<AppEvent>,
    ) -> Result<JoinHandle<Result<()>>> {
        let model = self.model.clone();

        Ok(tokio::spawn(async move {
            loop {
                let peripherals = adapter.peripherals().await?;
                let mut descriptors = Vec::new();
                for peripheral in &peripherals {
                    let address = peripheral.address();
                    if let Some(props) = peripheral.properties().await? {
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

#[async_trait]
impl BluetoothApi for BluetoothController {
    fn get_model(&self) -> Result<ModelHandle<dyn BluetoothModelApi>> {
        Ok(self.model.clone().into())
    }

    async fn discover_adapters(&mut self) -> Result<()> {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        let mut model = Vec::new();
        for adapter in adapters {
            let name = adapter.adapter_info().await.unwrap_or("unknown".into());
            let desc = AdapterDescriptor::new(name);
            model.push(desc.clone());
            self.adapters.insert(*desc.get_uuid(), adapter);
        }
        self.model.write().await.set_adapters(model);

        Ok(())
    }

    async fn select_peripheral(&self, uuid: &DeviceDescriptor) -> Result<()> {
        self.model.write().await.select_device(uuid.clone());
        Ok(())
    }

    async fn select_adapter(&self, adapter: &AdapterDescriptor) -> Result<()> {
        self.model
            .write()
            .await
            .select_adapter(adapter.get_uuid())?;
        Ok(())
    }

    async fn start_scan(&mut self) -> Result<()> {
        if self.model.read().await.is_scanning() {
            return Err(anyhow!("Already scanning"));
        }
        let handle = self.get_adapter().await?;
        handle.start_scan(ScanFilter::default()).await?;
        trace!(
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
    }

    async fn stop_scan(&mut self) -> Result<()> {
        if !self.model.read().await.is_scanning() {
            return Err(anyhow!("stop scan requested but no scan active"));
        }
        let handle = self.get_adapter().await?;
        handle.stop_scan().await?;
        trace!(
            "Stopped scanning on adapter {}.",
            handle.adapter_info().await?
        );
        if let Some(updater_handle) = &self.peri_updater_handle.take() {
            updater_handle.abort();
        }
        Ok(())
    }

    async fn start_listening(&mut self) -> Result<()> {
        let handle = self.get_adapter().await?;
        if let Some(jh) = &self.listener_handle {
            jh.abort();
        }

        let desc = self
            .model
            .write()
            .await
            .get_selected_device()
            .as_ref()
            .ok_or(anyhow!("no selected device!"))?
            .clone();
        self.listener_handle = Some(
            Self::listen_to_peripheral(handle.clone(), desc.address, self.event_bus.clone())
                .await?,
        );
        self.model.write().await.set_listening(Some(desc.address));
        Ok(())
    }

    async fn stop_listening(&mut self) -> Result<()> {
        if let Some(handle) = &self.listener_handle {
            handle.abort();
            self.model.write().await.set_listening(None);
        }
        Ok(())
    }
}
