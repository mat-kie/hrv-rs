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
use btleplug::api::{ ScanFilter};
use btleplug::{
    api::{BDAddr, Central, Manager as _},
    platform::{Adapter, Manager, Peripheral},
};

use anyhow::{anyhow, Result};
use btleplug::api::{Characteristic, ValueNotification};
use futures::stream::Stream;
use futures::StreamExt;
use log::{trace, warn};
use std::collections::HashMap;
use std::pin::Pin;
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

// Define a trait for the Bluetooth Adapter API
//#[cfg_attr(test, automock)]
#[async_trait]
pub trait BluetoothAdapterApi: Send + Sync {
    async fn start_scan(&self, filter: ScanFilter) -> Result<()>;
    async fn stop_scan(&self) -> Result<()>;
    async fn peripherals(&self) -> Result<Vec<Arc<dyn BluetoothPeripheralApi>>>;
    async fn get_name(&self) -> Result<String>;
    // Add other necessary methods
}

#[async_trait]
pub trait AdapterDiscovery<A: BluetoothAdapterApi> {
    async fn discover_adapters() -> Result<Vec<A>>;
}

// Define a trait for the Bluetooth Peripheral API
//#[cfg_attr(test, automock)]
#[async_trait]
pub trait BluetoothPeripheralApi: Send + Sync {
    fn address(&self) -> BDAddr;
    async fn connect(&self) -> Result<()>;
    async fn disconnect(&self) -> Result<()>;
    async fn discover_services(&self) -> Result<()>;
    fn characteristics(&self) -> Result<Vec<Characteristic>>;
    async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>>;
    async fn subscribe(&self, characteristic: &Characteristic) -> Result<()>;
    async fn get_name(&self) -> Result<String>;
}

#[async_trait]
impl BluetoothAdapterApi for Adapter {
    async fn start_scan(&self, filter: ScanFilter) -> Result<()> {
        Central::start_scan(self, filter)
            .await
            .map_err(|e| anyhow!(e))
    }

    async fn stop_scan(&self) -> Result<()> {
        Central::stop_scan(self).await.map_err(|e| anyhow!(e))
    }

    async fn peripherals(&self) -> Result<Vec<Arc<dyn BluetoothPeripheralApi>>> {
        let peripherals = Central::peripherals(self).await?;
        let wrapped: Vec<Arc<dyn BluetoothPeripheralApi>> = peripherals
            .into_iter()
            .map(|p| Arc::new(p) as Arc<dyn BluetoothPeripheralApi>)
            .collect();
        Ok(wrapped)
    }

    async fn get_name(&self) -> Result<String> {
        Ok(Central::adapter_info(self)
            .await
            .unwrap_or("unknown".to_string()))
    }
    // Implement other methods as needed
}

#[async_trait]
impl AdapterDiscovery<Adapter> for Adapter {
    async fn discover_adapters() -> Result<Vec<Adapter>> {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        Ok(adapters)
    }
}

#[async_trait]
impl BluetoothPeripheralApi for Peripheral {
    fn address(&self) -> BDAddr {
        btleplug::api::Peripheral::address(self)
    }

    async fn connect(&self) -> Result<()> {
        btleplug::api::Peripheral::connect(self)
            .await
            .map_err(|e| anyhow!(e))
    }

    async fn disconnect(&self) -> Result<()> {
        btleplug::api::Peripheral::disconnect(self)
            .await
            .map_err(|e| anyhow!(e))
    }

    async fn discover_services(&self) -> Result<()> {
        btleplug::api::Peripheral::discover_services(self)
            .await
            .map_err(|e| anyhow!(e))
    }

    fn characteristics(&self) -> Result<Vec<Characteristic>> {
        Ok(btleplug::api::Peripheral::characteristics(self)
            .into_iter()
            .collect())
    }

    async fn notifications(&self) -> Result<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>> {
        btleplug::api::Peripheral::notifications(self)
            .await
            .map_err(|e| anyhow!(e))
    }

    async fn subscribe(&self, characteristic: &Characteristic) -> Result<()> {
        btleplug::api::Peripheral::subscribe(self, characteristic)
            .await
            .map_err(|e| anyhow!(e))
    }
    async fn get_name(&self) -> Result<String> {
        btleplug::api::Peripheral::properties(self)
            .await
            .map(|p| p.unwrap().local_name.unwrap_or("unknown".to_string()))
            .map_err(|e| anyhow!(e))
    }
}

/// The Bluetooth Controller manages BLE interactions, including scanning for devices
/// and starting listening sessions.
pub struct BluetoothController<A: BluetoothAdapterApi + Clone + AdapterDiscovery<A> + 'static> {
    /// The Bluetooth model instance.
    model: Arc<RwLock<dyn BluetoothModelApi>>,
    event_bus: Sender<AppEvent>,
    /// Handle for the peripheral updater task.
    peri_updater_handle: Option<JoinHandle<Result<()>>>,
    /// Handle for the listener task.
    listener_handle: Option<JoinHandle<Result<()>>>,
    /// Event transmitter.
    adapters: HashMap<Uuid, A>, // Use trait object for adapter
}

impl<A: BluetoothAdapterApi + Clone + AdapterDiscovery<A>> BluetoothController<A> {
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
            adapters: HashMap::new(),
        }
    }

    /// Retrieves the selected Bluetooth adapter.
    ///
    /// # Returns
    /// A reference to the selected adapter, or an error if none is selected.
    async fn get_adapter(&self) -> Result<&A> {
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
        adapter: A,
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
            .characteristics()?
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
        adapter: A,
        _channel: Sender<AppEvent>,
    ) -> Result<JoinHandle<Result<()>>> {
        let model = self.model.clone();

        Ok(tokio::spawn(async move {
            loop {
                let peripherals = adapter.peripherals().await?;
                let mut descriptors = Vec::new();
                for peripheral in &peripherals {
                    let address = peripheral.address();
                    let name = peripheral.get_name().await?;
                    descriptors.push(DeviceDescriptor { name, address });
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
impl<A: BluetoothAdapterApi + Clone + AdapterDiscovery<A> + 'static> BluetoothApi
    for BluetoothController<A>
{
    fn get_model(&self) -> Result<ModelHandle<dyn BluetoothModelApi>> {
        Ok(self.model.clone().into())
    }

    async fn discover_adapters(&mut self) -> Result<()> {
        let mut model = Vec::new();
        for adapter in A::discover_adapters().await? {
            let name = adapter.get_name().await?;
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
        trace!("Scanning started on adapter {}.", handle.get_name().await?);
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
        trace!("Stopped scanning on adapter {}.", handle.get_name().await?);
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
#[cfg(test)]
    mod tests {
        use mockall::{mock, predicate::eq};
        use tokio::sync::broadcast;
        use anyhow::Result;
        use crate::model::bluetooth::MockBluetoothModelApi;

        use super::*;
        mock!{
            Adapter{}

            #[async_trait]
            impl BluetoothAdapterApi for Adapter{
                async fn start_scan(&self, filter: ScanFilter) -> Result<()>;
                async fn stop_scan(&self) -> Result<()>;
                async fn peripherals(&self) -> Result<Vec<Arc<dyn BluetoothPeripheralApi>>>;
                async fn get_name(&self) -> Result<String>;
            }

            #[async_trait]
            impl AdapterDiscovery<MockAdapter> for Adapter{
                async fn discover_adapters() -> Result<Vec<MockAdapter>>;
            }
            impl Clone for Adapter{
                fn clone(&self) -> Self;
            }
        }

        #[tokio::test]
        async fn test_discover_adapters() {
            let ctx = MockAdapter::discover_adapters_context();
            ctx.expect().times(1..).returning(||{
                let mut adapter = MockAdapter::new();
                adapter.expect_get_name().times(1..).returning( ||Ok("Test Adapter".to_string()));
                Ok(vec![adapter])
            } );
            let mut mock_model = MockBluetoothModelApi::new();
            mock_model.expect_set_adapters().return_const(());

            let (tx, _rx) = broadcast::channel(16);
            let model = Arc::new(RwLock::new(mock_model));
            let mut controller = BluetoothController::<MockAdapter>::new(model, tx);
            controller.discover_adapters().await.unwrap();
        }

        
    }