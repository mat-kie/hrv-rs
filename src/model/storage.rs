use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::Mutex;

use super::acquisition::AcquisitionModelApi;

#[typetag::serde(tag = "type")]
pub trait StorageModelApi {
    fn get_acquisitions(&self) -> &[Arc<Mutex<Box<dyn AcquisitionModelApi>>>];
    fn store_acquisition(&mut self, acq: Arc<Mutex<Box<dyn AcquisitionModelApi>>>);
    fn delete_acquisition(&mut self, idx: usize);
}

#[derive(Default, Clone)]
pub struct StorageModel {
    acquisitions: Vec<Arc<Mutex<Box<dyn AcquisitionModelApi>>>>,
}

// Custom implementation of Serialize
impl Serialize for StorageModel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the contents of `Arc<Mutex<dyn Trait>>`
        let acquisitions: Vec<_> = self
            .acquisitions
            .iter()
            .map(|arc_mutex: &Arc<Mutex<Box<dyn AcquisitionModelApi>>>| {
                arc_mutex.blocking_lock() // Lock the Mutex to access the inner value
            })
            .collect();
        // get the reference type for typetag serde
        let refs: Vec<_> = acquisitions.iter().map(|l| &**l).collect();
        refs.serialize(serializer)
    }
}

// Custom implementation of Deserialize
impl<'de> Deserialize<'de> for StorageModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let acquisitions: Vec<Box<dyn AcquisitionModelApi>> = Vec::deserialize(deserializer)?;
        Ok(StorageModel {
            acquisitions: acquisitions
                .into_iter()
                .map(|boxed_trait| Arc::new(Mutex::new(boxed_trait)))
                .collect(),
        })
    }
}

#[typetag::serde]
impl StorageModelApi for StorageModel {
    fn get_acquisitions(&self) -> &[Arc<Mutex<Box<dyn AcquisitionModelApi>>>] {
        &self.acquisitions
    }
    fn store_acquisition(&mut self, acq: Arc<Mutex<Box<dyn AcquisitionModelApi>>>) {
        self.acquisitions.push(acq);
    }
    fn delete_acquisition(&mut self, idx: usize) {
        if idx < self.acquisitions.len() {
            self.acquisitions.remove(idx);
        }
    }
}
