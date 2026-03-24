use std::hash::Hash;
use std::fmt::Debug;
use ahash::AHashMap;
use super::instances::{Instance, InstanceData, InstanceManager};

// Drain iterator implementation

pub struct Drain<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    instances: AHashMap<K, Instance<T>>,
    data: &'a mut Vec<InstanceData<T>>,
    free_head: &'a mut Option<usize>,
    keys: std::vec::IntoIter<K>,
}

impl<'a, K, T> Drain<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn new(manager: &'a mut InstanceManager<K, T>) -> Self {
        // Take ownership of instances map
        let instances = std::mem::take(&mut manager.instances);
        let keys: Vec<K> = instances.keys().copied().collect();
        
        Self {
            instances,
            data: &mut manager.data,
            free_head: &mut manager.free_head,
            keys: keys.into_iter(),
        }
    }
}

impl<'a, K, T> Iterator for Drain<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    type Item = (K, T);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(key) = self.keys.next() {
            if let Some(instance) = self.instances.remove(&key) {
                if let Some(index) = instance.instance_data_index {
                    if let Some(instance_data) = self.data.get_mut(index) {
                        if let Some(data) = instance_data.data.take() {
                            // Add the freed slot to the free list
                            instance_data.next = *self.free_head;
                            *self.free_head = Some(index);
                            return Some((key, data));
                        }
                    }
                }
            }
        }
        None
    }
}

impl<'a, K, T> Drop for Drain<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    fn drop(&mut self) {
        // Drain any remaining items and properly free their slots
        for _ in self {}
    }
}

pub struct OwnedDrain<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    instances: AHashMap<K, Instance<T>>,
    data: Vec<InstanceData<T>>,
    keys: std::vec::IntoIter<K>,
}

impl<K, T> OwnedDrain<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn new(instances: AHashMap<K, Instance<T>>, data: Vec<InstanceData<T>>) -> Self {
        let keys: Vec<K> = instances.keys().copied().collect();
        Self {
            instances,
            data,
            keys: keys.into_iter(),
        }
    }
}

impl<K, T> Iterator for OwnedDrain<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    type Item = (K, T);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(key) = self.keys.next() {
            if let Some(instance) = self.instances.remove(&key) {
                if let Some(index) = instance.instance_data_index {
                    if let Some(instance_data) = self.data.get_mut(index) {
                        if let Some(data) = instance_data.data.take() {
                            return Some((key, data));
                        }
                    }
                }
            }
        }
        None
    }
}