use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::{RwLock, RwLockReadGuard};

use super::acquisition::AcquisitionModelApi;

pub trait StorageModelApi: Sync + Send  {
    type AcqModelType: AcquisitionModelApi + Serialize + DeserializeOwned;
    fn get_acquisitions(&self) -> &[ModelHandle<dyn AcquisitionModelApi>];
    fn get_mut_acquisitions(&self) -> &[Arc<RwLock<Self::AcqModelType>>];
    fn store_acquisition(&mut self, acq: Arc<RwLock<Self::AcqModelType>>);
    fn delete_acquisition(&mut self, idx: usize);
}

#[derive(Default, Clone)]
pub struct StorageModel< AMT: AcquisitionModelApi> {
    acquisitions: Vec<Arc<RwLock<AMT>>>,
    handles: Vec<ModelHandle<dyn AcquisitionModelApi>>
}

// Custom implementation of Serialize
impl<AMT: AcquisitionModelApi + Serialize> Serialize for StorageModel<AMT> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the contents of `Arc<RwLock<dyn Trait>>`
        let acquisitions: Vec<_> = self
            .acquisitions
            .iter()
            .map(|arc_rwlock| {
                arc_rwlock.blocking_read() // Lock the RwLock to access the inner value
            })
            .collect();
        // get the reference type for typetag serde
        let refs: Vec<_> = acquisitions.iter().map(|l| &**l).collect();
        refs.serialize(serializer)
    }
}

// Custom implementation of Deserialize
impl<'a,  AMT: AcquisitionModelApi + Deserialize<'a> + 'static> Deserialize<'a> for StorageModel<AMT> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        let acquisitions: Vec<AMT> = Vec::deserialize(deserializer)?;
        let acquisitions:Vec<Arc<RwLock<AMT>>> = acquisitions.into_iter()
        .map(|boxed_trait| Arc::new(RwLock::new(boxed_trait)))
        .collect();

        let handles: Vec<ModelHandle<dyn AcquisitionModelApi>> = acquisitions.
        iter()
        .map(|amt|{let d: Arc<RwLock<dyn AcquisitionModelApi>> = amt.clone(); ModelHandle::<dyn AcquisitionModelApi>::from(d)}).collect();

        Ok(StorageModel {
            acquisitions,
            handles
        })
    }
}


impl<AMT: AcquisitionModelApi + Serialize + DeserializeOwned + 'static> StorageModelApi for StorageModel< AMT> {

    type AcqModelType = AMT;

    fn get_mut_acquisitions(&self) -> &[Arc<RwLock<Self::AcqModelType>>] {
        &self.acquisitions
    }
    fn get_acquisitions(&self) -> &[ModelHandle<dyn AcquisitionModelApi>] {
        &self.handles
    }
    fn store_acquisition(&mut self, acq: Arc<RwLock<Self::AcqModelType>>)
    {

        self.acquisitions.push(acq.clone());
        self.handles.push((acq as  Arc<RwLock<dyn AcquisitionModelApi>>).into());

    }
    fn delete_acquisition(&mut self, idx: usize) {
        if idx < self.acquisitions.len() {
            self.acquisitions.remove(idx);
            self.handles.remove(idx);
        }
    }
}

pub struct ModelHandle<T:?Sized>{
    data:Arc<RwLock<T>>
}

impl<T:?Sized> Clone for ModelHandle<T>{
    fn clone(&self) -> Self {
        Self{data:self.data.clone()}
    }
}

impl<T: ?Sized> From<Arc<RwLock<T>>> for ModelHandle<T>{
    fn from(value: Arc<RwLock<T>>) -> Self {
        Self { data: value }
    }
}


impl<T:?Sized> ModelHandle<T>{
    pub async fn read(&self)->RwLockReadGuard<'_,T>{
        self.data.read().await
    }
    pub fn blocking_read(&self)->RwLockReadGuard<'_,T>{
        self.data.blocking_read()
    }
}
