//! Bluetooth Model
//!
//! This module defines the model and utility structures for managing Bluetooth-related data.
//! It provides abstractions for:
//! - Bluetooth Low Energy (BLE) Heart Rate Service (HRS) messages
//! - Device and adapter management
//! - Scanning and connection state tracking

use btleplug::api::BDAddr;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
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

impl Hash for AdapterDescriptor {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl PartialEq for AdapterDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.uuid.eq(&other.uuid)
    }
}
impl Eq for AdapterDescriptor {}

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

impl PartialOrd for AdapterDescriptor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.uuid.partial_cmp(&other.uuid)
    }
}
