//! Bluetooth Model
//!
//! This module defines the model and utility structures for managing Bluetooth-related data.
//! It provides abstractions for:
//! - Bluetooth Low Energy (BLE) Heart Rate Service (HRS) messages
//! - Device and adapter management
//! - Scanning and connection state tracking

use anyhow::{anyhow, Result};
use btleplug::api::BDAddr;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Debug;
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
/// Parses and stores data from the Heart Rate Service including:
/// - Heart rate value (8 or 16 bit)
/// - RR intervals (time between beats)
/// - Energy expenditure
/// - Sensor contact status
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

/// Represents a discovered Bluetooth device.
///
/// Contains:
/// - Device name (if available)
/// - Bluetooth address (MAC)
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceDescriptor {
    /// The name of the device, if available.
    pub name: String,
    /// The unique Bluetooth address of the device.
    pub address: BDAddr,
}

/// Represents a Bluetooth adapter with a unique identifier.
///
/// Stores information about a Bluetooth adapter including:
/// - A human-readable name
/// - A unique UUID for identification
#[derive(Clone, Debug)]
pub struct AdapterDescriptor {
    name: String,
    uuid: Uuid,
}
impl AdapterDescriptor {
    pub fn new(name: String) -> Self {
        Self {
            name,
            uuid: Uuid::new_v4(),
        }
    }
    pub fn get_name(&self) -> &str {
        &self.name
    }
    pub fn get_uuid(&self) -> &Uuid {
        &self.uuid
    }
}
impl PartialEq for AdapterDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.uuid.eq(&other.uuid)
    }
}
impl PartialOrd for AdapterDescriptor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.uuid.partial_cmp(&other.uuid)
    }
}

/// API for managing Bluetooth-related data.
///
/// This trait provides methods for:
/// - Managing Bluetooth adapters and their selection
/// - Tracking discovered devices
/// - Managing device scanning and connection states
pub trait BluetoothModelApi: Debug + Send + Sync {
    /// Gets the list of Bluetooth adapters as a vector of `(Name, UUID)` tuples.
    ///
    /// # Returns
    /// A vector of tuples containing adapter names and UUIDs.
    fn get_adapters(&self) -> &[AdapterDescriptor];

    /// Sets the list of Bluetooth adapters.
    ///
    /// # Arguments
    /// * `adapters` - A vector of adapters.
    fn set_adapters(&mut self, adapters: Vec<AdapterDescriptor>);

    /// Gets the currently selected adapter, if any.
    ///
    /// # Returns
    /// An optional reference to the selected adapter.
    fn get_selected_adapter(&self) -> &Option<AdapterDescriptor>;

    /// Selects a Bluetooth adapter by its UUID.
    ///
    /// # Arguments
    /// * `uuid` - The UUID of the adapter to select.
    ///
    /// # Returns
    /// `Ok(())` if the adapter was successfully selected, or `Err(String)` if the UUID was not found.
    fn select_adapter(&mut self, uuid: &Uuid) -> Result<()>;

    /// Gets the list of discovered Bluetooth devices.
    ///
    /// # Returns
    /// A reference to the vector of devices.
    fn get_devices(&self) -> &Vec<DeviceDescriptor>;

    /// Clears the list of discovered devices.
    #[allow(dead_code)]
    fn clear_devices(&mut self);

    /// Sets the list of discovered devices.
    ///
    /// # Arguments
    /// * `devices` - A vector of `(BDAddr, String)` tuples representing the devices.
    fn set_devices(&mut self, devices: Vec<DeviceDescriptor>);

    fn select_device(&mut self, device: DeviceDescriptor);
    fn get_selected_device(&self) -> &Option<DeviceDescriptor>;

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
    #[allow(dead_code)]
    fn is_listening_to(&self) -> &Option<BDAddr>;
    fn set_listening(&mut self, device: Option<BDAddr>);
}

/// Default implementation of the Bluetooth model.
///
/// Manages:
/// - Available Bluetooth adapters
/// - Currently selected adapter and device
/// - List of discovered devices
/// - Scanning and connection states
#[derive(Debug, Default)]
pub struct BluetoothModel {
    adapters: Vec<AdapterDescriptor>,
    selected_adapter: Option<AdapterDescriptor>,
    selected_device: Option<DeviceDescriptor>,
    devices: Vec<DeviceDescriptor>,
    scanning: bool,
    listening: Option<BDAddr>,
}

impl BluetoothModelApi for BluetoothModel {
    fn select_device(&mut self, device: DeviceDescriptor) {
        self.selected_device = Some(device)
    }
    fn get_selected_device(&self) -> &Option<DeviceDescriptor> {
        &self.selected_device
    }
    fn get_adapters(&self) -> &[AdapterDescriptor] {
        &self.adapters
    }

    fn set_adapters(&mut self, adapters: Vec<AdapterDescriptor>) {
        let mut sorted_adapters = adapters;
        sorted_adapters.sort_by(|a, b| a.get_uuid().cmp(b.get_uuid()));
        self.adapters = sorted_adapters;
    }

    fn get_selected_adapter(&self) -> &Option<AdapterDescriptor> {
        &self.selected_adapter
    }

    fn select_adapter(&mut self, uuid: &Uuid) -> Result<()> {
        if let Ok(idx) = self
            .adapters
            .binary_search_by(|adapter| adapter.get_uuid().cmp(uuid))
        {
            self.selected_adapter = Some(self.adapters[idx].clone());
            Ok(())
        } else {
            Err(anyhow!("Could not find an adapter for UUID: {}", uuid))
        }
    }

    fn get_devices(&self) -> &Vec<DeviceDescriptor> {
        &self.devices
    }

    fn clear_devices(&mut self) {
        self.devices.clear();
    }

    fn set_devices(&mut self, devices: Vec<DeviceDescriptor>) {
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

    #[test]
    fn test_hr_service_msg_no_rr_intervals() {
        // Short HR, no energy expenditure, no sensor contact, no RR intervals
        let data = [0b00000000, 75];
        let msg = HeartrateMessage::new(&data);
        assert_eq!(msg.get_hr(), 75.0);
        assert!(!msg.has_rr_interval());
        assert_eq!(msg.get_rr_intervals(), &[] as &[u16]);
    }

    #[test]
    fn test_hr_service_msg_with_sensor_contact() {
        // Short HR, no energy expenditure, sensor contact, no RR intervals
        let data = [0b00000110, 72];
        let msg = HeartrateMessage::new(&data);
        assert_eq!(msg.get_hr(), 72.0);
        assert!(msg.sen_has_contact());
        assert!(msg.sen_contact_supported());
    }

    #[test]
    fn test_hr_service_msg_with_long_hr_and_energy_exp() {
        // Long HR, energy expenditure, no sensor contact, no RR intervals
        let data = [0b00001001, 90, 1, 10, 0];
        let msg = HeartrateMessage::new(&data);
        assert_eq!(msg.get_hr(), 346.0); // 90 + (1 << 8)
        assert!(msg.has_energy_exp());
        assert_eq!(msg.get_energy_exp(), 10.0);
    }

    #[test]
    fn test_hr_service_msg_with_all_flags() {
        // Long HR, energy expenditure, sensor contact, RR intervals
        let data = [0b00011111, 100, 0, 5, 0, 0, 4, 0, 1];
        let msg = HeartrateMessage::new(&data);
        assert_eq!(msg.get_hr(), 100.0);
        assert!(msg.has_energy_exp());
        assert_eq!(msg.get_energy_exp(), 5.0);
        assert!(msg.has_rr_interval());
        assert_eq!(msg.get_rr_intervals(), &[1000, 250]);
        assert!(msg.sen_has_contact());
        assert!(msg.sen_contact_supported());
    }
}
