//! Core Events
//!
//! This module defines events used for communication between different components
//! of the HRV analysis tool. Events are central to the application's event-driven architecture.

use anyhow::Result;
use event_bridge::EventBridge;
use std::path::PathBuf;

use crate::{
    api::controller::{BluetoothApi, MeasurementApi, OutlierFilter, RecordingApi, StorageEventApi},
    model::bluetooth::{AdapterDescriptor, DeviceDescriptor, HeartrateMessage},
};

#[derive(Debug, Clone, EventBridge)]
#[forward_to_trait(StorageEventApi)]
#[trait_returned_type(HandlerResult)]
pub enum StorageEvent {
    Clear,
    LoadFromFile(PathBuf),
    StoreToFile(PathBuf),
}

#[derive(Debug, Clone, EventBridge)]
#[forward_to_trait(MeasurementApi)]
#[trait_returned_type(HandlerResult)]
pub enum MeasurementEvent {
    SetStatsWindow(usize),
    SetOutlierFilter(OutlierFilter),
    RecordMessage(HeartrateMessage),
}

#[derive(Debug, Clone, EventBridge)]
#[forward_to_trait(RecordingApi)]
#[trait_returned_type(HandlerResult)]
pub enum RecordingEvent {
    StartRecording,
    StopRecording,
}

type HandlerResult = Result<()>;
#[derive(Debug, Clone, EventBridge)]
#[forward_to_trait(BluetoothApi)]
#[trait_returned_type(HandlerResult)]
pub enum BluetoothEvent {
    SelectAdapter(AdapterDescriptor),
    SelectPeripheral(DeviceDescriptor),
    //StartScan,
    //StopScan,
}

#[derive(Debug, Clone)]
pub enum StateChangeEvent {
    DiscardRecording,
    StoreRecording,
    ToRecordingState,
    InitialState,
    SelectMeasurement(usize),
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Storage(StorageEvent),
    Bluetooth(BluetoothEvent),
    Recording(RecordingEvent),
    Measurement(MeasurementEvent),
    AppState(StateChangeEvent),
}
