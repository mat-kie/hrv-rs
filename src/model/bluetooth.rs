//! Bluetooth Model
//!
//! This module defines the model and utility structures for managing Bluetooth-related data in the HRV analysis tool.
//! It provides abstractions for interacting with Bluetooth adapters, devices, and Heart Rate Service (HRS) messages.

use crate::core::constants::HEARTRATE_MEASUREMENT_UUID;
use crate::core::events::{AppEvent, HrvEvent};
use crate::map_err;
use btleplug::api::{Peripheral, ScanFilter};
use btleplug::{
    api::{BDAddr, Central, Manager as _},
    platform::{Adapter, Manager},
};
use futures::StreamExt;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Helper macro to check if a specific bit is set in a byte.
macro_rules! is_bit_set {
    ($byte:expr, $pos:expr) => {
        ($byte & (1 << $pos)) != 0
    };
}

/// Helper macro to extract a `u16` value in little-endian format from a byte slice.
macro_rules! get_u16_little_endian {
    ($slice:expr, $lsb:expr) => {
        (($slice[1 + $lsb] as u16) << 8) | $slice[$lsb] as u16
    };
}

/// Represents a Bluetooth LE Heart Rate Service (HRS) message.
///
/// The HRS message contains heart rate data, energy expenditure, and RR intervals
/// (time between successive heartbeats).
#[derive(Copy, Clone, Default, Deserialize, Serialize, Debug, PartialEq)]
pub struct HeartrateMessage {
    /// Flags indicating the presence of optional data and data encoding.
    flags: u8,
    /// Heart rate value in beats per minute (BPM).
    hr_value: u16,
    /// Energy expenditure in kilojoules (if present).
    energy_expended: u16,
    /// Array of RR interval values in milliseconds (if present).
    rr_values: [u16; 9],
}

impl HeartrateMessage {
    /// Constructs a new `HeartrateMessage` from raw data.
    ///
    /// # Arguments
    /// * `data` - A byte slice containing the raw HRS message data.
    ///
    /// # Panics
    /// Panics if the provided data slice is too short.
    pub fn new(data: &[u8]) -> Self {
        assert!(
            data.len() > 1,
            "Invalid length: data must contain at least 2 bytes."
        );

        let flags = data[0];
        let hr_value = if is_bit_set!(flags, 0) && data.len() >= 3 {
            get_u16_little_endian!(data, 1)
        } else {
            data[1] as u16
        };

        let mut result = HeartrateMessage {
            flags,
            hr_value,
            energy_expended: 0,
            rr_values: [0u16; 9],
        };

        if result.has_energy_exp() {
            result.energy_expended = get_u16_little_endian!(data, result.energy_exp_offset());
        }

        let rr_offset = result.rr_offset();
        for (rr_store, chunk) in result.rr_values.iter_mut().zip(data[rr_offset..].chunks(2)) {
            *rr_store = (get_u16_little_endian!(chunk, 0) as f64 * 1000f64 / 1024f64) as u16;
        }

        result
    }

    /// Checks if the heart rate value uses 16-bit representation.
    pub fn has_long_hr(&self) -> bool {
        is_bit_set!(self.flags, 0)
    }

    /// Returns the heart rate value as a floating-point number.
    pub fn get_hr(&self) -> f64 {
        self.hr_value.into()
    }

    /// Checks if RR intervals are present.
    pub fn has_rr_interval(&self) -> bool {
        is_bit_set!(self.flags, 4)
    }

    /// Returns a slice of the valid RR intervals.
    pub fn get_rr_intervals(&self) -> &[u16] {
        let count = self.rr_values.iter().take_while(|&&x| x != 0).count();
        &self.rr_values[..count]
    }

    /// Checks if energy expenditure data is available.
    pub fn has_energy_exp(&self) -> bool {
        is_bit_set!(self.flags, 3)
    }

    /// Returns the energy expenditure value in kilojoules.
    pub fn get_energy_exp(&self) -> f64 {
        self.energy_expended as f64
    }

    /// Checks if the sensor has contact with the user's body.
    pub fn sen_has_contact(&self) -> bool {
        is_bit_set!(self.flags, 1)
    }

    /// Checks if the sensor supports contact detection.
    pub fn sen_contact_supported(&self) -> bool {
        is_bit_set!(self.flags, 2)
    }

    /// Returns the offset for energy expenditure data in the raw message.
    fn energy_exp_offset(&self) -> usize {
        2 + (self.has_long_hr() as usize)
    }

    /// Returns the offset for RR interval data in the raw message.
    fn rr_offset(&self) -> usize {
        if self.has_energy_exp() {
            self.energy_exp_offset() + 2
        } else {
            self.energy_exp_offset()
        }
    }
}

impl fmt::Display for HeartrateMessage {
    /// Formats the HRS message as a human-readable string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "============================")?;
        writeln!(f, "Heart Rate Service Message:")?;
        writeln!(f, "----------------------------")?;
        writeln!(f, "Flags: 0b{:08b}", self.flags)?;
        writeln!(f, "Has Long HR: {}", self.has_long_hr())?;
        writeln!(f, "Heart Rate Value: {:.2}", self.get_hr())?;
        writeln!(f, "Has RR Interval: {}", self.has_rr_interval())?;

        if self.has_rr_interval() {
            let rr_intervals = self
                .get_rr_intervals()
                .iter()
                .map(|rr| format!("{:.2} ms", rr))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "RR Intervals: [{}]", rr_intervals)?;
        } else {
            writeln!(f, "RR Intervals: None")?;
        }

        writeln!(f, "Has Energy Expended: {}", self.has_energy_exp())?;
        if self.has_energy_exp() {
            writeln!(f, "Energy Expended: {} kJ", self.get_energy_exp())?;
        }
        writeln!(f, "Sensor Has Contact: {}", self.sen_has_contact())?;
        writeln!(
            f,
            "Sensor Contact Supported: {}",
            self.sen_contact_supported()
        )
    }
}

/// Represents a descriptor for a Bluetooth device.
///
/// This structure stores the basic details of a discovered Bluetooth device,
/// including its name and address.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceDescriptor {
    /// The name of the device, if available.
    pub name: String,
    /// The unique Bluetooth address of the device.
    pub address: BDAddr,
}

/// A trait representing a handle for interacting with Bluetooth adapters.
///
/// This trait provides a unified interface for performing Bluetooth operations,
/// such as scanning for peripherals, managing connections, and retrieving adapter information.
/// It abstracts over `btleplug`'s `Central` implementation for flexible usage and testing.
pub trait AdapterHandle: PartialOrd + Clone + Send + Sync {
    /// The type of the underlying `Central` implementation for this adapter.
    type CentralType: Send + Sync;

    // --- Adapter Metadata ---

    /// Creates a new adapter handle.
    ///
    /// # Arguments
    /// * `adapter` - The `Central` instance representing the Bluetooth adapter.
    /// * `name` - A human-readable name for the adapter.
    ///
    /// # Returns
    /// A new instance of the implementing type.
    fn new(adapter: Self::CentralType, name: &str) -> Self;

    /// Retrieves the human-readable name of the adapter.
    ///
    /// # Returns
    /// A reference to the adapter's name.
    fn name(&self) -> &str;

    /// Retrieves the UUID of the adapter.
    ///
    /// # Returns
    /// A reference to the adapter's UUID.
    fn uuid(&self) -> &Uuid;

    // --- Scanning Operations ---

    /// Starts scanning for Bluetooth peripherals using the adapter.
    ///
    /// # Returns
    /// A `Future` that resolves to `Result<(), String>`, where an error string indicates failure.
    fn start_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    /// Stops scanning for Bluetooth peripherals using the adapter.
    ///
    /// # Returns
    /// A `Future` that resolves to `Result<(), btleplug::Error>`, indicating success or failure.
    fn stop_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    // --- Peripheral Operations ---

    /// Initiates listening to a specific peripheral.
    ///
    /// # Arguments
    /// * `peripheral` - The Bluetooth address of the target peripheral.
    /// * `tx` - A sender channel for broadcasting events related to the peripheral.
    ///
    /// # Returns
    /// A `Future` that resolves to `Result<JoinHandle<()>, btleplug::Error>`.
    /// The `JoinHandle` represents the asynchronous task managing the peripheral connection.
    #[allow(clippy::type_complexity)]
    fn listen_to_peripheral<'a>(
        &'a self,
        peripheral: BDAddr,
        tx: Sender<AppEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<JoinHandle<Result<(), String>>, String>> + Send + 'a>>;

    /// Retrieves the list of peripherals discovered by the adapter.
    ///
    /// # Returns
    /// A `Future` that resolves to `Result<Vec<DeviceDescriptor>, btleplug::Error>`.
    /// The `DeviceDescriptor` provides details about each discovered peripheral.
    fn peripherals<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DeviceDescriptor>, String>> + Send + 'a>>;

    // --- Adapter Discovery and Information ---

    /// Retrieves a list of available adapters on the system.
    ///
    /// # Returns
    /// A `Future` that resolves to `Result<Vec<Self>, String>`, where the vector contains
    /// instances of the implementing type representing each discovered adapter.
    fn retrieve_adapters<'a>(
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Self>, String>> + Send + 'a>>;
}

/// Represents a Bluetooth adapter.
///
/// This structure provides metadata and operations for a Bluetooth adapter,
/// including its name, UUID, and an interface to the `btleplug` `Central` API.
#[derive(Clone, Debug)]
pub struct BluetoothAdapter {
    /// The name of the adapter.
    name: String,
    /// The unique UUID of the adapter.
    uuid: Uuid,
    /// The `btleplug` adapter instance.
    adapter: Adapter,
}

impl PartialEq for BluetoothAdapter {
    fn eq(&self, other: &Self) -> bool {
        self.uuid.eq(&other.uuid)
    }
}

impl PartialOrd for BluetoothAdapter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.uuid.partial_cmp(&other.uuid)
    }
}

impl AdapterHandle for BluetoothAdapter {
    type CentralType = Adapter;

    fn new(adapter: Self::CentralType, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            uuid: Uuid::new_v4(),
            adapter,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn retrieve_adapters<'a>(
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Self>, String>> + Send + 'a>> {
        Box::pin(async {
            let manager = Manager::new().await.map_err(|e| e.to_string())?;
            let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
            let mut res = Vec::new();
            for adapter in adapters {
                let name = adapter.adapter_info().await.unwrap_or("unknown".into());
                res.push(Self::new(adapter, &name));
            }
            Ok(res)
        })
    }

    fn start_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { map_err!(self.adapter.start_scan(ScanFilter::default()).await) })
    }

    fn stop_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { map_err!(self.adapter.stop_scan().await) })
    }

    fn peripherals<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DeviceDescriptor>, String>> + Send + 'a>> {
        Box::pin(async {
            let peris = map_err!(self.adapter.peripherals().await)?;
            let mut descriptors = Vec::new();
            for peri in &peris {
                let address = peri.address();
                if let Some(props) = map_err!(peri.properties().await)? {
                    if let Some(name) = props.local_name {
                        descriptors.push(DeviceDescriptor { name, address });
                    }
                }
            }
            Ok(descriptors)
        })
    }

    fn listen_to_peripheral<'a>(
        &'a self,
        peripheral_address: BDAddr,
        tx: Sender<AppEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<JoinHandle<Result<(), String>>, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let peripherals = map_err!(self.adapter.peripherals().await)?;
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
                            .send(AppEvent::Data(HrvEvent::HrMessage(HeartrateMessage::new(
                                &data.value,
                            ))))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                Err("listener terminated".into())
            });
            Ok(fut)
        })
    }
}

#[derive(Clone, Debug)]
pub struct MockAdapterHandle {
    name: String,
    uuid: Uuid,
    peripherals: Arc<Mutex<Vec<DeviceDescriptor>>>,
}
impl PartialEq for MockAdapterHandle {
    fn eq(&self, other: &Self) -> bool {
        self.uuid.eq(&other.uuid)
    }
}
impl PartialOrd for MockAdapterHandle {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.uuid.partial_cmp(&other.uuid)
    }
}
impl AdapterHandle for MockAdapterHandle {
    type CentralType = ();

    fn new(_: Self::CentralType, name: &str) -> Self {
        Self {
            name: name.to_string(),
            uuid: Uuid::new_v4(),
            peripherals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn start_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        let peripherals = self.peripherals.clone();
        Box::pin(async move {
            let mut peripherals_lock = peripherals.lock().await;
            peripherals_lock.push(DeviceDescriptor {
                name: "Mock Device 1".to_string(),
                address: [0, 1, 2, 3, 4, 5].into(),
            });
            peripherals_lock.push(DeviceDescriptor {
                name: "Mock Device 2".to_string(),
                address: [5, 4, 3, 2, 1, 0].into(),
            });
            Ok(())
        })
    }

    fn stop_scan<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            // Mock stopping the scan
            Ok(())
        })
    }

    fn listen_to_peripheral<'a>(
        &'a self,
        peripheral: BDAddr,
        _tx: Sender<AppEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<JoinHandle<Result<(), String>>, String>> + Send + 'a>>
    {
        Box::pin(async move {
            if peripheral == BDAddr::from([0, 1, 2, 3, 4, 5]) {
                Ok(tokio::spawn(async move {
                    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
                    loop {
                        let val: u16 = 900 + rng.gen_range(0..200);
                        let data: [u8; 4] =
                            [0b10000, 60, (val & 255) as _, ((val >> 8) & 255) as _];
                        let _ = _tx
                            .send(AppEvent::Data(HrvEvent::HrMessage(HeartrateMessage::new(
                                &data,
                            ))))
                            .await;

                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }))
            } else {
                Err("Peripheral not found".to_string())
            }
        })
    }

    fn peripherals<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DeviceDescriptor>, String>> + Send + 'a>> {
        let peripherals = self.peripherals.clone();
        Box::pin(async move {
            let peripherals_lock = peripherals.lock().await;
            Ok(peripherals_lock.clone())
        })
    }

    fn retrieve_adapters<'a>(
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Self>, String>> + Send + 'a>> {
        Box::pin(async move {
            Ok(vec![Self {
                name: "Mock Adapter".to_string(),
                uuid: Uuid::new_v4(),
                peripherals: Arc::new(Mutex::new(Vec::new())),
            }])
        })
    }
}

/// API for managing Bluetooth-related data.
///
/// This trait abstracts the management of Bluetooth adapters, discovered devices,
/// and scanning status.
pub trait BluetoothModelApi<AHT: AdapterHandle>: Debug + Send {
    /// Gets the list of Bluetooth adapters as a vector of `(Name, UUID)` tuples.
    ///
    /// # Returns
    /// A vector of tuples containing adapter names and UUIDs.
    fn get_adapter_names(&self) -> Vec<(String, Uuid)>;

    /// Sets the list of Bluetooth adapters.
    ///
    /// # Arguments
    /// * `adapters` - A vector of adapters.
    fn set_adapters(&mut self, adapters: Vec<AHT>);

    /// Gets the currently selected adapter, if any.
    ///
    /// # Returns
    /// An optional reference to the selected adapter.
    fn get_selected_adapter(&self) -> &Option<AHT>;

    /// Selects a Bluetooth adapter by its UUID.
    ///
    /// # Arguments
    /// * `uuid` - The UUID of the adapter to select.
    ///
    /// # Returns
    /// `Ok(())` if the adapter was successfully selected, or `Err(String)` if the UUID was not found.
    fn select_adapter(&mut self, uuid: &Uuid) -> Result<(), String>;

    /// Gets the list of discovered Bluetooth devices.
    ///
    /// # Returns
    /// A reference to the vector of devices.
    fn get_devices(&self) -> &Vec<(BDAddr, String)>;

    /// Clears the list of discovered devices.
    #[allow(dead_code)]
    fn clear_devices(&mut self);

    /// Sets the list of discovered devices.
    ///
    /// # Arguments
    /// * `devices` - A vector of `(BDAddr, String)` tuples representing the devices.
    fn set_devices(&mut self, devices: Vec<(BDAddr, String)>);

    /// Gets the scanning status.
    ///
    /// # Returns
    /// `true` if scanning is active, `false` otherwise.
    fn is_scanning(&self) -> bool;

    /// Sets the scanning status.
    ///
    /// # Arguments
    /// * `status` - `true` if scanning is active, `false` otherwise.
    #[allow(dead_code)]
    fn set_scanning(&mut self, status: bool);

    fn is_listening_to(&self) -> &Option<BDAddr>;
    fn set_listening(&mut self, device: Option<BDAddr>);
}

/// Default implementation of the `BTModelApi` trait for managing Bluetooth data.
#[derive(Debug)]
pub struct BluetoothModel<AdapterHandleType: AdapterHandle + Send> {
    adapters: Vec<AdapterHandleType>,
    selected_adapter: Option<AdapterHandleType>,
    devices: Vec<(BDAddr, String)>,
    scanning: bool,
    listening: Option<BDAddr>,
}
impl<AdapterHandleType: AdapterHandle> Default for BluetoothModel<AdapterHandleType> {
    fn default() -> Self {
        Self {
            adapters: Vec::<AdapterHandleType>::new(),
            selected_adapter: None,
            devices: Vec::new(),
            scanning: false,
            listening: None,
        }
    }
}

impl<AdapterHandleType: AdapterHandle + Debug + Send> BluetoothModelApi<AdapterHandleType>
    for BluetoothModel<AdapterHandleType>
{
    fn get_adapter_names(&self) -> Vec<(String, Uuid)> {
        self.adapters
            .iter()
            .map(|entry| (entry.name().to_owned(), *entry.uuid()))
            .collect()
    }

    fn set_adapters(&mut self, adapters: Vec<AdapterHandleType>) {
        let mut sorted_adapters = adapters;
        sorted_adapters.sort_by(|a, b| a.uuid().cmp(b.uuid()));
        self.adapters = sorted_adapters;
    }

    fn get_selected_adapter(&self) -> &Option<AdapterHandleType> {
        &self.selected_adapter
    }

    fn select_adapter(&mut self, uuid: &Uuid) -> Result<(), String> {
        if let Ok(idx) = self
            .adapters
            .binary_search_by(|adapter| adapter.uuid().cmp(uuid))
        {
            self.selected_adapter = Some(self.adapters[idx].clone());
            Ok(())
        } else {
            Err(format!("Could not find an adapter for UUID: {}", uuid))
        }
    }

    fn get_devices(&self) -> &Vec<(BDAddr, String)> {
        &self.devices
    }

    fn clear_devices(&mut self) {
        self.devices.clear();
    }

    fn set_devices(&mut self, devices: Vec<(BDAddr, String)>) {
        self.devices = devices;
    }

    fn is_scanning(&self) -> bool {
        self.scanning
    }

    fn set_scanning(&mut self, status: bool) {
        self.scanning = status;
    }
    fn is_listening_to(&self) -> &Option<BDAddr> {
        &self.listening
    }
    fn set_listening(&mut self, device: Option<BDAddr>) {
        self.listening = device;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hr_service_msg_short_hr_no_exp() {
        // Short HR, no energy expenditure, no sensor contact, RR intervals (1024 and 256)
        let data = [0b00010000, 80, 0, 4, 0, 1];
        let msg = HeartrateMessage::new(&data);
        assert_eq!(msg.get_hr(), 80.0);
        assert!(msg.has_rr_interval());
        assert_eq!(msg.get_rr_intervals(), &[1000, 250]);
    }

    #[test]
    fn test_hr_service_msg_long_hr_no_exp() {
        // Long HR, no energy expenditure, no sensor contact, RR intervals (1024 and 256)
        let data = [0b00010001, 80, 0, 0, 4, 0, 1];
        let msg = HeartrateMessage::new(&data);

        // Verify flags and HR value
        assert_eq!(msg.flags, 0b00010001);
        assert_eq!(msg.get_hr(), 80.0);
        assert!(!msg.sen_contact_supported());

        // Verify RR intervals
        assert!(msg.has_rr_interval());
        assert_eq!(msg.get_rr_intervals(), &[1000, 250]);
    }

    #[test]
    fn test_hr_service_msg_with_energy_exp() {
        // Short HR, energy expenditure, no sensor contact, RR intervals (1024 and 256)
        let data = [0b00011001, 80, 0, 1, 2, 0, 4, 0, 1];
        let msg = HeartrateMessage::new(&data);

        // Verify flags and HR value
        assert_eq!(msg.flags, 0b00011001);
        assert_eq!(msg.get_hr(), 80.0);

        // Verify energy expenditure
        assert!(msg.has_energy_exp());
        assert_eq!(msg.get_energy_exp(), 513.0);

        // Verify RR intervals
        assert!(msg.has_rr_interval());
        assert_eq!(msg.get_rr_intervals(), &[1000, 250]);
    }

    #[test]
    #[should_panic(expected = "Invalid length")]
    fn test_invalid_data_length() {
        HeartrateMessage::new(&[0b00000001]);
    }

    #[test]
    fn test_display_trait() {
        let data = [0b00011001, 80, 0, 42, 1, 0, 4, 128, 0];
        let msg = HeartrateMessage::new(&data);
        let output = format!("{}", msg);
        assert!(output.contains("Heart Rate Value: 80.00"));
    }
}
