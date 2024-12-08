//! Bluetooth Controller Module
//!
//! This module implements the Bluetooth Low Energy (BLE) functionality for HRV measurements.
//! It handles device discovery, connection management, and data acquisition from BLE heart rate monitors.
use crate::api::controller::{AdapterDiscovery, BluetoothApi, DisplayName, RecordingApi};
use crate::api::model::BluetoothModelApi;
use crate::core::constants::HEARTRATE_MEASUREMENT_UUID;
use crate::core::events::{AppEvent, MeasurementEvent};
use crate::model::bluetooth::AdapterDescriptor;
use crate::model::bluetooth::{DeviceDescriptor, HeartrateMessage};
use anyhow::{anyhow, Result};
use btleplug::api::Peripheral;
use btleplug::{
    api::{BDAddr, Central, Manager as _},
    platform::{Adapter, Manager},
};

use futures::StreamExt;
use log::{trace, warn};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use uuid::Uuid;

use async_trait::async_trait;

/// Manages Bluetooth operations and state.
///
/// # Type Parameters
/// - `A`: Bluetooth adapter type that implements required traits
///
/// # Fields
/// - `event_bus`: Channel for broadcasting Bluetooth-related events
/// - `peri_updater_handle`: Task handle for peripheral device updates
/// - `listener_handle`: Task handle for BLE notifications
/// - `adapter_descriptors`: List of available Bluetooth adapters as their descriptors
/// - `adapters`: Map of adapter UUIDs to adapter instances
/// - `selected_adapter`: Currently selected adapter and its descriptor
/// - `selected_device`: Currently selected BLE device
/// - `devices`: Thread-safe list of discovered devices
/// - `scanning`: Indicates if device scanning is active
/// - `listening`: Address of device currently being monitored
#[derive(Debug)]
pub struct BluetoothComponent<A: Central + DisplayName + AdapterDiscovery<A> + 'static>
where
    A::Peripheral: DisplayName,
{
    event_bus: Sender<AppEvent>,
    peri_updater_handle: Option<JoinHandle<Result<()>>>,
    listener_handle: Option<JoinHandle<Result<()>>>,
    adapter_descriptors: Vec<AdapterDescriptor>,
    adapters: HashMap<Uuid, A>,
    selected_adapter: Option<(AdapterDescriptor, A)>,
    selected_device: Option<DeviceDescriptor>,
    devices: Arc<RwLock<Vec<DeviceDescriptor>>>,
    scanning: bool,
    listening: Option<BDAddr>,
}

impl<A: DisplayName + Central + AdapterDiscovery<A>> Drop for BluetoothComponent<A>
where
    <A as Central>::Peripheral: DisplayName,
{
    /// Ensures proper cleanup of Bluetooth resources when component is dropped.
    /// Stops any active scanning operations.
    fn drop(&mut self) {
        if let Some(handle) = &self.peri_updater_handle {
            handle.abort();
        }
        if let Some(handle) = &self.listener_handle {
            handle.abort();
        }
    }
}

impl<A: DisplayName + Central + AdapterDiscovery<A>> BluetoothComponent<A>
where
    <A as Central>::Peripheral: DisplayName,
{
    /// Creates a new `BluetoothController` instance.
    ///
    /// # Arguments
    /// - `event_bus`: The event bus for broadcasting application events.
    ///
    /// # Returns
    /// A new `BluetoothController` instance.
    pub fn new(event_bus: Sender<AppEvent>) -> Self {
        Self {
            event_bus,
            peri_updater_handle: None,
            listener_handle: None,
            adapter_descriptors: Vec::new(),
            adapters: HashMap::new(),
            selected_adapter: None,
            selected_device: None,
            devices: Arc::new(RwLock::new(Vec::new())),
            scanning: false,
            listening: None,
        }
    }
    pub async fn listen_to_peripheral(
        adapter: A,
        peripheral_address: BDAddr,
        tx: Sender<AppEvent>,
    ) -> Result<JoinHandle<Result<()>>> {
        let fut = tokio::spawn(async move {
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
            while let Some(data) = notification_stream.next().await {
                if data.value.len() < 2
                    || tx
                        .send(AppEvent::Measurement(MeasurementEvent::RecordMessage(
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
}

#[async_trait]
impl<A: Central + DisplayName + AdapterDiscovery<A> + Debug + 'static> BluetoothApi
    for BluetoothComponent<A>
where
    A::Peripheral: DisplayName,
{
    async fn discover_adapters(&mut self) -> Result<()> {
        for adapter in A::discover_adapters().await? {
            let name = adapter.get_name().await?;
            let desc = AdapterDescriptor::new(name);
            self.adapter_descriptors.push(desc.clone());
            self.adapters.insert(*desc.get_uuid(), adapter);
        }
        self.adapter_descriptors
            .sort_by(|a, b| a.get_uuid().cmp(b.get_uuid()));

        Ok(())
    }

    async fn select_peripheral(&mut self, dev: DeviceDescriptor) -> Result<()> {
        self.selected_device = Some(dev);
        Ok(())
    }

    async fn select_adapter(&mut self, adapter: AdapterDescriptor) -> Result<()> {
        let (uuid, handle) = self
            .adapters
            .get_key_value(adapter.get_uuid())
            .ok_or(anyhow!("Adapter not found"))?;
        let desc = self
            .adapter_descriptors
            .iter()
            .find(|d| d.get_uuid() == uuid)
            .ok_or(anyhow!("Adapter not found"))?;
        self.selected_adapter = Some((desc.clone(), handle.clone()));
        self.start_scan().await
    }

    async fn start_scan(&mut self) -> Result<()> {
        if self.scanning {
            return Err(anyhow!("Already scanning"));
        }
        let adapter = self
            .selected_adapter
            .as_ref()
            .ok_or(anyhow!("no selected adapter!"))?
            .1
            .clone();
        trace!("Scanning started on adapter {}.", adapter.get_name().await?);
        let devices = self.devices.clone();
        if self.peri_updater_handle.is_none() {
            self.peri_updater_handle = Some(tokio::spawn(async move {
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
                    *devices.write().await = descriptors;
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }));
        }
        Ok(())
    }

    async fn stop_scan(&mut self) -> Result<()> {
        if !self.scanning {
            return Err(anyhow!("stop scan requested but no scan active"));
        }
        if let Some((_, adapter)) = self.selected_adapter.as_ref() {
            adapter.stop_scan().await?;
            trace!("Stopped scanning on adapter {}.", adapter.get_name().await?);
        } else {
            return Err(anyhow!("no selected adapter!"));
        }
        if let Some(updater_handle) = &self.peri_updater_handle.take() {
            updater_handle.abort();
        }
        self.scanning = false;
        Ok(())
    }

    async fn start_listening(&mut self) -> Result<()> {
        let (_, adapter) = &self
            .selected_adapter
            .as_ref()
            .ok_or(anyhow!("no selected adapter!"))?;
        if let Some(jh) = &self.listener_handle {
            jh.abort();
        }

        let desc = self
            .selected_device
            .as_ref()
            .ok_or(anyhow!("no selected device!"))?
            .clone();
        self.listener_handle = Some(
            BluetoothComponent::listen_to_peripheral(
                adapter.clone(),
                desc.address,
                self.event_bus.clone(),
            )
            .await?,
        );
        self.listening = Some(desc.address);
        Ok(())
    }

    async fn stop_listening(&mut self) -> Result<()> {
        if let Some(handle) = &self.listener_handle {
            handle.abort();
            self.listening = None;
        }
        Ok(())
    }
}

impl<A: Central + DisplayName + AdapterDiscovery<A> + Debug + 'static> BluetoothModelApi
    for BluetoothComponent<A>
where
    A::Peripheral: DisplayName,
{
    fn get_selected_device(&self) -> Option<DeviceDescriptor> {
        self.selected_device.clone()
    }
    fn get_adapters(&self) -> &[AdapterDescriptor] {
        self.adapter_descriptors.as_slice()
    }

    fn get_selected_adapter(&self) -> Option<AdapterDescriptor> {
        if let Some((desc, _)) = self.selected_adapter.as_ref() {
            Some(desc.clone())
        } else {
            None
        }
    }

    fn get_devices(&self) -> &Arc<RwLock<Vec<DeviceDescriptor>>> {
        &self.devices
    }

    fn is_scanning(&self) -> bool {
        self.scanning
    }

    fn is_listening_to(&self) -> Option<BDAddr> {
        self.listening
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

#[async_trait]
impl<A: Central + DisplayName + AdapterDiscovery<A> + Debug + 'static> RecordingApi
    for BluetoothComponent<A>
where
    A::Peripheral: DisplayName,
{
    async fn start_recording(&mut self) -> Result<()> {
        self.start_listening().await
    }
    async fn stop_recording(&mut self) -> Result<()> {
        self.stop_listening().await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use btleplug::{
        api::{
            Central, CentralEvent, CentralState, Characteristic, Descriptor, PeripheralProperties,
            ScanFilter, Service, ValueNotification, WriteType,
        },
        platform::PeripheralId,
    };
    use futures::stream::Stream;
    use mockall::mock;

    use std::{collections::BTreeSet, pin::Pin};
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

        impl std::fmt::Debug for Adapter {
            fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
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
    async fn test_new_bluetooth_component() {
        let (tx, _rx) = broadcast::channel(16);
        let component = BluetoothComponent::<MockAdapter>::new(tx);
        assert!(component.selected_adapter.is_none());
        assert!(component.selected_device.is_none());
        assert!(component.devices.read().await.is_empty());
        assert!(!component.scanning);
        assert!(component.listening.is_none());
    }

    #[tokio::test]
    async fn test_discover_adapters() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);
        let _da_ctx = MockAdapter::discover_adapters_context();
        _da_ctx.expect().times(1).returning(|| {
            let mut adapter = MockAdapter::default();
            adapter
                .expect_get_name()
                .once()
                .returning(|| Ok("MockAdapter".to_string()));
            Ok(vec![adapter])
        });
        assert!(component.discover_adapters().await.is_ok());
        assert!(!component.adapter_descriptors.is_empty());
        assert_eq!(
            component.adapter_descriptors.first().unwrap().get_name(),
            "MockAdapter"
        );
    }

    #[tokio::test]
    async fn test_select_adapter() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);
        component
            .adapter_descriptors
            .push(AdapterDescriptor::new("MockAdapter".to_string()));
        let mut adapter = MockAdapter::default();
        let _ = adapter.expect_get_name();
        let _ = adapter.expect_start_scan().returning(|_| Ok(()));
        let _ = adapter.expect_clone().returning(|| {
            let mut adapter = MockAdapter::default();
            adapter.expect_clone().returning(|| {
                let mut adapter = MockAdapter::default();
                adapter
                    .expect_get_name()
                    .returning(|| Ok("MockAdapter".to_string()));
                adapter.expect_peripherals().returning(|| Ok(vec![]));
                adapter
            });
            adapter
                .expect_get_name()
                .returning(|| Ok("MockAdapter".to_string()));
            adapter.expect_peripherals().returning(|| Ok(vec![]));
            adapter
        });
        component.adapters.insert(
            *component.adapter_descriptors.first().unwrap().get_uuid(),
            adapter,
        );
        assert!(component
            .select_adapter(component.adapter_descriptors.first().unwrap().clone())
            .await
            .is_ok());
        assert!(component.get_selected_adapter().is_some());
    }
    #[tokio::test]
    async fn test_select_peripheral() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);
        let device = DeviceDescriptor {
            name: "TestDevice".to_string(),
            address: BDAddr::default(),
        };
        assert!(component.select_peripheral(device.clone()).await.is_ok());
        assert_eq!(component.get_selected_device().unwrap(), device);
    }

    #[tokio::test]
    async fn test_start_listening() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);

        // Setup adapter
        let mut adapter = MockAdapter::default();
        adapter.expect_clone().returning(|| {
            let mut adapter = MockAdapter::default();
            adapter
                .expect_get_name()
                .returning(|| Ok("MockAdapter".to_string()));
            adapter.expect_peripherals().returning(|| {
                let mut peripheral = MockPeripheral::default();
                peripheral.expect_address().returning(BDAddr::default);
                peripheral.expect_connect().returning(|| Ok(()));
                peripheral.expect_discover_services().returning(|| Ok(()));
                peripheral.expect_characteristics().returning(|| {
                    let mut chars = BTreeSet::new();
                    chars.insert(Characteristic {
                        uuid: HEARTRATE_MEASUREMENT_UUID,
                        service_uuid: Uuid::nil(),
                        descriptors: BTreeSet::new(),
                        properties: Default::default(),
                    });
                    chars
                });
                peripheral.expect_subscribe().returning(|_| Ok(()));
                peripheral
                    .expect_notifications()
                    .returning(|| Ok(Box::pin(futures::stream::empty())));
                Ok(vec![peripheral])
            });
            adapter
        });

        // Setup test state
        let desc = AdapterDescriptor::new("MockAdapter".to_string());
        component.selected_adapter = Some((desc.clone(), adapter));
        component.selected_device = Some(DeviceDescriptor {
            name: "TestDevice".to_string(),
            address: BDAddr::default(),
        });

        assert!(component.start_listening().await.is_ok());
        assert!(component.listening.is_some());
    }

    #[tokio::test]
    async fn test_stop_listening() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);
        component.listening = Some(BDAddr::default());

        // Create dummy task handle
        let handle = tokio::spawn(async { Ok::<(), anyhow::Error>(()) });
        component.listener_handle = Some(handle);

        assert!(component.stop_listening().await.is_ok());
        assert!(component.listening.is_none());
    }

    #[tokio::test]
    async fn test_stop_scan() {
        let (tx, _rx) = broadcast::channel(16);
        let mut component = BluetoothComponent::<MockAdapter>::new(tx);

        let mut adapter = MockAdapter::default();
        adapter.expect_stop_scan().returning(|| Ok(()));
        adapter
            .expect_get_name()
            .returning(|| Ok("MockAdapter".to_string()));

        component.scanning = true;
        component.selected_adapter =
            Some((AdapterDescriptor::new("MockAdapter".to_string()), adapter));

        assert!(component.stop_scan().await.is_ok());
        assert!(!component.scanning);
    }
}
