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

use futures::StreamExt;
use log::{trace, warn};
use std::collections::HashMap;

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

#[async_trait]
pub trait AdapterDiscovery<A: Central + DisplayName>
where
    A::Peripheral: DisplayName,
{
    async fn discover_adapters() -> Result<Vec<A>>;
}

#[async_trait]
pub trait DisplayName {
    async fn get_name(&self) -> Result<String>;
}
/// The Bluetooth Controller manages BLE interactions, including scanning for devices
/// and starting listening sessions.
pub struct BluetoothController<A: Central + DisplayName + AdapterDiscovery<A> + 'static>
where
    A::Peripheral: DisplayName,
{
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

impl<A: Central + AdapterDiscovery<A> + DisplayName> BluetoothController<A>
where
    A::Peripheral: DisplayName,
{
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
                    if let Ok(name) = peripheral.get_name().await {
                        descriptors.push(DeviceDescriptor { name, address });
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
impl<A: Central + DisplayName + AdapterDiscovery<A> + 'static> BluetoothApi
    for BluetoothController<A>
where
    A::Peripheral: DisplayName,
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

#[async_trait]
impl DisplayName for Adapter {
    async fn get_name(&self) -> Result<String> {
        Ok(self.adapter_info().await?)
    }
}

#[async_trait]
impl AdapterDiscovery<Adapter> for Adapter {
    async fn discover_adapters() -> Result<Vec<Adapter>> {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        let mut result = Vec::new();
        for adapter in adapters {
            result.push(adapter);
        }
        Ok(result)
    }
}

#[async_trait]
impl DisplayName for btleplug::platform::Peripheral {
    async fn get_name(&self) -> Result<String> {
        if let Some(props) = self.properties().await? {
            if let Some(name) = props.local_name {
                return Ok(name);
            }
        }
        Err(anyhow!("No name found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::bluetooth::MockBluetoothModelApi;
    use anyhow::Result;
    use btleplug::api::Central;
    use btleplug::api::{Characteristic, ValueNotification};
    use btleplug::{
        api::{CentralEvent, CentralState, Descriptor, PeripheralProperties, Service, WriteType},
        platform::PeripheralId,
    };
    use futures::stream::Stream;
    use mockall::mock;
    use std::collections::BTreeSet;
    use std::pin::Pin;
    use tokio::sync::broadcast;

    mock! {
        Peripheral{}

        impl Clone for Peripheral {
            fn clone(&self) -> Self;
        }

        impl std::fmt::Debug for Peripheral {
            fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
        }

        #[async_trait]
        impl Peripheral for Peripheral {
            fn id(&self) -> PeripheralId;
            fn address(&self) -> BDAddr;
            async fn properties(&self) -> btleplug::Result<Option<PeripheralProperties>>;
            fn services(&self) -> BTreeSet<Service>;
            fn characteristics(&self) -> BTreeSet<Characteristic> {
                self.services()
                    .iter()
                    .flat_map(|service| service.characteristics.clone().into_iter())
                    .collect()
            }
            async fn is_connected(&self) -> btleplug::Result<bool>;
            async fn connect(&self) -> btleplug::Result<()>;
            async fn disconnect(&self) -> btleplug::Result<()>;
            async fn discover_services(&self) -> btleplug::Result<()>;
            async fn write(
                &self,
                characteristic: &Characteristic,
                data: &[u8],
                write_type: WriteType,
            ) -> btleplug::Result<()>;
            async fn read(&self, characteristic: &Characteristic) -> btleplug::Result<Vec<u8>>;
            async fn subscribe(&self, characteristic: &Characteristic) -> btleplug::Result<()>;
            async fn unsubscribe(&self, characteristic: &Characteristic) -> btleplug::Result<()>;
            async fn notifications(&self) -> btleplug::Result<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>>;
            async fn write_descriptor(&self, descriptor: &Descriptor, data: &[u8]) -> btleplug::Result<()>;
            async fn read_descriptor(&self, descriptor: &Descriptor) -> btleplug::Result<Vec<u8>>;
        }
        #[async_trait]
        impl DisplayName for Peripheral {
            async fn get_name(&self) -> Result<String>;
        }
    }

    mock! {
        Adapter{}
        impl Clone for Adapter {
            fn clone(&self) -> Self;
        }

        #[async_trait]
        impl DisplayName for Adapter {
            async fn get_name(&self) -> Result<String>;
        }

        #[async_trait]
        impl AdapterDiscovery<MockAdapter> for Adapter {
            async fn discover_adapters() -> Result<Vec<MockAdapter>>;
        }

        #[async_trait]
        impl Central for Adapter {
            type Peripheral = MockPeripheral;

            async fn events(&self) -> btleplug::Result<Pin<Box<dyn Stream<Item = CentralEvent> + Send>>>;

            async fn start_scan(&self, filter: ScanFilter) -> btleplug::Result<()>;

            async fn stop_scan(&self) -> btleplug::Result<()>;

            async fn peripherals(&self) -> btleplug::Result<Vec<MockPeripheral>>;

            async fn peripheral(&self, id: &PeripheralId) -> btleplug::Result<MockPeripheral>;

            async fn add_peripheral(&self, address: &PeripheralId) -> btleplug::Result<MockPeripheral>;

            async fn adapter_info(&self) -> btleplug::Result<String>;

            async fn adapter_state(&self) -> btleplug::Result<CentralState>;
        }
    }

    #[tokio::test]
    async fn test_discover_adapters_success() {
        let ctx = MockAdapter::discover_adapters_context();
        ctx.expect().times(1..).returning(|| {
            let mut adapter = MockAdapter::new();
            adapter
                .expect_get_name()
                .times(1..)
                .returning(|| Ok("Test Adapter".to_string()));
            Ok(vec![adapter])
        });
        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_set_adapters().return_const(());

        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let mut controller = BluetoothController::<MockAdapter>::new(model, tx);

        assert!(controller.discover_adapters().await.is_ok());
    }
    #[tokio::test]
    async fn test_select_adapter() {
        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_select_adapter().returning(|_x| Ok(()));
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let controller = BluetoothController::<MockAdapter>::new(model, tx);

        let adapter_desc = AdapterDescriptor::new("Test Adapter".to_string());
        controller.select_adapter(&adapter_desc).await.unwrap();
    }
    #[tokio::test]
    async fn test_start_scan() {
        let mut mock_adapter = MockAdapter::new();
        mock_adapter.expect_start_scan().returning(|_| Ok(()));
        mock_adapter
            .expect_get_name()
            .returning(|| Ok("Test Adapter".to_string()));
        mock_adapter.expect_peripherals().returning(|| Ok(vec![]));
        mock_adapter.expect_clone().returning(|| {
            let mut adapter = MockAdapter::new();
            adapter
                .expect_get_name()
                .times(1..)
                .returning(|| Ok("Test Adapter".to_string()));
            adapter
        });

        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_is_scanning().return_const(false);
        mock_model.expect_set_devices().return_const(());
        mock_model.expect_set_adapters().return_const(());

        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let mut controller = BluetoothController::<MockAdapter>::new(model.clone(), tx);
        let desc = Uuid::new_v4();
        let ad = AdapterDescriptor::new_with_uuid("Test Adapter".to_string(), desc);
        controller.adapters.insert(desc, mock_adapter);
        model
            .write()
            .await
            .expect_get_selected_adapter()
            .return_const(Some(ad));

        controller.start_scan().await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_scan() {
        let mut mock_adapter = MockAdapter::new();
        mock_adapter.expect_stop_scan().returning(|| Ok(()));
        mock_adapter
            .expect_get_name()
            .returning(|| Ok("Test Adapter".to_string()));
        mock_adapter.expect_peripherals().returning(|| Ok(vec![]));
        mock_adapter.expect_clone().returning(|| {
            let mut adapter = MockAdapter::new();
            adapter
                .expect_get_name()
                .times(1..)
                .returning(|| Ok("Test Adapter".to_string()));
            adapter
        });

        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_is_scanning().return_const(true);
        mock_model.expect_set_devices().return_const(());
        mock_model.expect_set_adapters().return_const(());

        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let mut controller = BluetoothController::<MockAdapter>::new(model.clone(), tx);
        let desc = Uuid::new_v4();
        let ad = AdapterDescriptor::new_with_uuid("Test Adapter".to_string(), desc);
        controller.adapters.insert(desc, mock_adapter);
        model
            .write()
            .await
            .expect_get_selected_adapter()
            .return_const(Some(ad));

        controller.stop_scan().await.unwrap();
    }

    #[tokio::test]
    async fn test_select_peripheral() {
        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_select_device().return_const(());

        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let controller = BluetoothController::<MockAdapter>::new(model, tx);

        let device_desc = DeviceDescriptor {
            name: "Test Device".to_string(),
            address: BDAddr::from_str_delim("00:11:22:33:44:55").unwrap(),
        };
        controller.select_peripheral(&device_desc).await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_listening() {
        let mut mock_model = MockBluetoothModelApi::new();
        mock_model.expect_set_listening().return_const(());

        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(mock_model));
        let mut controller = BluetoothController::<MockAdapter>::new(model, tx);

        controller.stop_listening().await.unwrap();
    }
}
