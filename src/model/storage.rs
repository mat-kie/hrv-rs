//! Storage Module
//!
//! This module defines the storage system for managing acquisitions in the HRV analysis tool.
//! It provides traits and structs for storing and retrieving acquisition models.
//!
//! The `StorageModelApi` trait defines the interface for storage models,
//! and `StorageModel` is a default implementation that uses an in-memory vector to store acquisitions.

use std::sync::Arc;

use mockall::automock;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::{RwLock, RwLockReadGuard};

use super::acquisition::AcquisitionModel;
use super::acquisition::AcquisitionModelApi;

/// Trait defining the interface for storage models.
///
/// This trait allows for managing a collection of acquisition models,
/// providing methods to access, store, and delete acquisitions.
#[automock(type AcqModelType = AcquisitionModel;)]
pub trait StorageModelApi: Sync + Send {
    /// The type of acquisition model being stored, which must implement `AcquisitionModelApi`,
    /// `Serialize`, and `DeserializeOwned`.
    type AcqModelType: AcquisitionModelApi + Serialize + DeserializeOwned;

    /// Returns a slice of handles to the stored acquisition models.
    fn get_acquisitions(&self) -> &[ModelHandle<dyn AcquisitionModelApi>];

    /// Returns a mutable slice of the stored acquisition models.
    fn get_mut_acquisitions(&self) -> &[Arc<RwLock<Self::AcqModelType>>];

    /// Stores a new acquisition.
    ///
    /// # Arguments
    ///
    /// * `acq` - An `Arc<RwLock<Self::AcqModelType>>` representing the acquisition to store.
    fn store_acquisition(&mut self, acq: Arc<RwLock<Self::AcqModelType>>);

    /// Deletes an acquisition at the specified index.
    ///
    /// # Arguments
    ///
    /// * `idx` - The index of the acquisition to delete.
    #[allow(dead_code)]
    fn delete_acquisition(&mut self, idx: usize);
}

/// Default implementation of the `StorageModelApi` trait.
///
/// This struct uses an in-memory vector to store acquisitions.
#[derive(Default, Clone, Debug)]
pub struct StorageModel<AMT: AcquisitionModelApi> {
    /// Vector of stored acquisitions.
    acquisitions: Vec<Arc<RwLock<AMT>>>,

    /// Vector of handles to the stored acquisitions.
    handles: Vec<ModelHandle<dyn AcquisitionModelApi>>,
}

// Custom implementation of `Serialize`
impl<AMT: AcquisitionModelApi + Serialize> Serialize for StorageModel<AMT> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the contents of `Arc<RwLock<AMT>>`
        let acquisitions: Vec<_> = self
            .acquisitions
            .iter()
            .map(|arc_rwlock| arc_rwlock.blocking_read())
            .collect();
        // Get references for serialization
        let refs: Vec<_> = acquisitions.iter().map(|l| &**l).collect();
        refs.serialize(serializer)
    }
}

// Custom implementation of `Deserialize`
impl<'de, AMT> Deserialize<'de> for StorageModel<AMT>
where
    AMT: AcquisitionModelApi + DeserializeOwned + 'static,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let acquisitions: Vec<AMT> = Vec::deserialize(deserializer)?;
        let acquisitions: Vec<Arc<RwLock<AMT>>> = acquisitions
            .into_iter()
            .map(|acq| Arc::new(RwLock::new(acq)))
            .collect();

        let handles: Vec<ModelHandle<dyn AcquisitionModelApi>> = acquisitions
            .iter()
            .map(|amt| {
                let d: Arc<RwLock<dyn AcquisitionModelApi>> = amt.clone();
                ModelHandle::<dyn AcquisitionModelApi>::from(d)
            })
            .collect();

        Ok(StorageModel {
            acquisitions,
            handles,
        })
    }
}

impl<AMT> StorageModelApi for StorageModel<AMT>
where
    AMT: AcquisitionModelApi + Serialize + DeserializeOwned + 'static,
{
    type AcqModelType = AMT;

    fn get_mut_acquisitions(&self) -> &[Arc<RwLock<Self::AcqModelType>>] {
        &self.acquisitions
    }

    fn get_acquisitions(&self) -> &[ModelHandle<dyn AcquisitionModelApi>] {
        &self.handles
    }

    fn store_acquisition(&mut self, acq: Arc<RwLock<Self::AcqModelType>>) {
        self.acquisitions.push(acq.clone());
        self.handles
            .push((acq as Arc<RwLock<dyn AcquisitionModelApi>>).into());
    }

    fn delete_acquisition(&mut self, idx: usize) {
        if idx < self.acquisitions.len() {
            self.acquisitions.remove(idx);
            self.handles.remove(idx);
        }
    }
}

/// A handle to a model, wrapping an `Arc<RwLock<T>>`.
#[derive(Debug)]
pub struct ModelHandle<T: ?Sized> {
    /// The shared data wrapped in an `Arc<RwLock<T>>`.
    data: Arc<RwLock<T>>,
}

impl<T: ?Sized> Clone for ModelHandle<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
        }
    }
}

impl<T: ?Sized> From<Arc<RwLock<T>>> for ModelHandle<T> {
    fn from(value: Arc<RwLock<T>>) -> Self {
        Self { data: value }
    }
}

impl<T: ?Sized> ModelHandle<T> {
    /// Asynchronously acquires a read lock on the data.
    ///
    /// # Returns
    ///
    /// An `RwLockReadGuard` that allows read access to the data.
    #[allow(dead_code)]
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.data.read().await
    }

    /// Synchronously acquires a read lock on the data.
    ///
    /// # Returns
    ///
    /// An `RwLockReadGuard` that allows read access to the data.
    pub fn blocking_read(&self) -> RwLockReadGuard<'_, T> {
        self.data.blocking_read()
    }
}
