use std::hash::Hash;
use std::fmt::Debug;
use std::marker::PhantomData;
use ahash::AHashMap;

use crate::drain::{Drain, OwnedDrain};

#[derive(Debug, Clone)]
pub struct Instance<T> {
    pub instance_data_index: Option<usize>,
    _marker: PhantomData<T>,
}

// Note: Cannot derive Copy for InstanceData<T> because T might not be Copy
#[derive(Debug)]
pub struct InstanceData<T> {
    pub data: Option<T>,
    pub next: Option<usize>,
}

// Only implement Clone when T: Clone
impl<T: Clone> Clone for InstanceData<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            next: self.next,
        }
    }
}

// Only implement Copy when T: Copy
impl<T: Copy> Copy for InstanceData<T> {}

#[derive(Debug)]
pub struct InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub instances: AHashMap<K, Instance<T>>,
    pub data: Vec<InstanceData<T>>,
    pub free_head: Option<usize>,
}

impl<K, T> Default for InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, T> InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    /// Creates a new, empty InstanceManager.
    pub fn new() -> Self {
        InstanceManager {
            instances: AHashMap::new(),
            data: Vec::new(),
            free_head: None,
        }
    }

    /// Creates a new InstanceManager with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        InstanceManager {
            instances: AHashMap::with_capacity(capacity),
            data: Vec::with_capacity(capacity),
            free_head: None,
        }
    }

    /// Returns the number of active instances
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Returns true if the manager contains no instances
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Returns the total capacity of the data vector
    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    /// Creates or updates an instance, returning a mutable reference to the data
    #[inline(always)]
    pub fn create_or_update(&mut self, id: K, data: T) -> Option<&mut T> {
        if self.is_available(id) {
            self.create(id, data)
        } else {
            self.update(id, data)
        }
    }

    /// Inserts data for a key, returning the previous value if it existed
    pub fn insert(&mut self, id: K, data: T) -> Option<T> {
        if let Some(old_data) = self.remove(id) {
            self.create(id, data);
            Some(old_data)
        } else {
            self.create(id, data);
            None
        }
    }

    /// Checks if an instance with the given ID exists
    #[inline(always)]
    pub fn has(&self, id: K) -> bool {
        self.instances.contains_key(&id)
    }

    /// Alias for `has` - more idiomatic Rust naming
    #[inline(always)]
    pub fn contains_key(&self, id: &K) -> bool {
        self.instances.contains_key(id)
    }

    /// Gets an immutable reference to the data for the given ID
    #[inline(always)]
    pub fn get(&self, id: K) -> Option<&T> {
        let instance = self.instances.get(&id)?;
        let index = instance.instance_data_index?;
        let instance_data = self.data.get(index)?;
        instance_data.data.as_ref()
    }

    /// Gets a mutable reference to the data for the given ID
    #[inline(always)]
    pub fn get_mut(&mut self, id: K) -> Option<&mut T> {
        let instance = self.instances.get(&id)?;
        let index = instance.instance_data_index?;
        let instance_data = self.data.get_mut(index)?;
        instance_data.data.as_mut()
    }

    /// Removes and returns the data for the given ID
    #[inline(always)]
    pub fn remove(&mut self, id: K) -> Option<T> {
        self.pop(id)
    }

    /// Removes and returns the data for the given ID (original method name)
    #[inline(always)]
    pub fn pop(&mut self, id: K) -> Option<T> {
        // Remove the instance
        let instance = self.instances.remove(&id)?;
        
        // Get the index
        let index = instance.instance_data_index?;
        
        // Take the data out of the slot
        let instance_data = self.data.get_mut(index)?;
        let value = instance_data.data.take()?;
        
        // Recycle the slot
        instance_data.next = self.free_head;
        self.free_head = Some(index);

        Some(value)
    }

    /// Sets the data for an existing instance
    #[inline(always)]
    pub fn set_data(&mut self, id: K, data: T) -> Option<()> {
        let instance = self.instances.get(&id)?;
        let index = instance.instance_data_index?;
        let instance_data = self.data.get_mut(index)?;
        instance_data.data = Some(data);
        Some(())
    }

    /// Returns an iterator over all key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.instances.iter().filter_map(move |(k, instance)| {
            let index = instance.instance_data_index?;
            let instance_data = self.data.get(index)?;
            let data = instance_data.data.as_ref()?;
            Some((*k, data))
        })
    }

    /// Returns a mutable iterator over all key-value pairs
    pub fn iter_mut(&mut self) -> IterMut<'_, K, T> {
        IterMut {
            instances: &self.instances,
            data: &mut self.data,
            keys: self.instances.keys().copied().collect::<Vec<_>>().into_iter(),
        }
    }

    /// Returns an iterator over all keys
    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.instances.keys().copied()
    }

    /// Returns an iterator over all values
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.instances.values().filter_map(move |instance| {
            let index = instance.instance_data_index?;
            let instance_data = self.data.get(index)?;
            instance_data.data.as_ref()
        })
    }

    /// Returns a mutable iterator over all values
    pub fn values_mut(&mut self) -> ValuesMut<'_, T> {
        ValuesMut {
            data: &mut self.data,
            indices: self.instances.values()
                .filter_map(|instance| instance.instance_data_index)
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }

    /// Reserves capacity for at least `additional` more instances
    pub fn reserve(&mut self, additional: usize) {
        self.instances.reserve(additional);
        self.data.reserve(additional);
    }

    /// Shrinks the capacity as much as possible
    pub fn shrink_to_fit(&mut self) {
        self.instances.shrink_to_fit();
        self.data.shrink_to_fit();
    }

    /// Retains only the instances for which the predicate returns true
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(K, &mut T) -> bool,
    {
        // Collect keys that might be removed (candidates)
        let mut candidates = Vec::new();
        for (key, instance) in &self.instances {
            if let Some(index) = instance.instance_data_index {
                if let Some(instance_data) = self.data.get(index) {
                    if instance_data.data.is_some() {
                        candidates.push(*key);
                    }
                }
            }
        }
        
        // Filter candidates: keep keys where f returns true, remove those where false
        let keys_to_remove: Vec<K> = candidates
            .into_iter()
            .filter(|&key| {
                match self.get_mut(key) {
                    Some(data_ref) => !f(key, data_ref), // remove if f returns false
                    None => true,                        // remove if no data
                }
            })
            .collect();
            
        // Remove the keys
        for key in keys_to_remove {
            self.remove(key);
        }
    }

    /// Removes all instances that match the predicate
    pub fn remove_if<F>(&mut self, mut f: F) 
    where
        F: FnMut(K, &T) -> bool,
    {
        let keys_to_remove: Vec<K> = self.instances
            .iter()
            .filter_map(|(key, instance)| {
                let index = instance.instance_data_index?;
                let instance_data = self.data.get(index)?;
                let data = instance_data.data.as_ref()?;
                if f(*key, data) {
                    Some(*key)
                } else {
                    None
                }
            })
            .collect();
            
        for key in keys_to_remove {
            self.remove(key);
        }
    }

    /// Applies a function to all instances, modifying them in place
    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(K, &mut T),
    {
        // Using the same pattern as retain for safe iteration
        let candidates: Vec<K> = self.instances
            .iter()
            .filter_map(|(key, instance)| {
                let index = instance.instance_data_index?;
                let instance_data = self.data.get(index)?;
                if instance_data.data.is_some() {
                    Some(*key)
                } else {
                    None
                }
            })
            .collect();
            
        for key in candidates {
            if let Some(data_ref) = self.get_mut(key) {
                f(key, data_ref);
            }
        }
    }

    /// Checks if the ID is available (not currently used)
    #[inline(always)]
    fn is_available(&self, id: K) -> bool {
        !self.instances.contains_key(&id)
    }

    /// Creates a new instance with the given data
    #[inline(always)]
    fn create(&mut self, id: K, data: T) -> Option<&mut T> {
        let index = self.get_free_slot();

        self.data[index].data = Some(data);

        let instance = Instance {
            instance_data_index: Some(index),
            _marker: PhantomData,
        };

        self.instances.insert(id, instance);

        self.data.get_mut(index)?.data.as_mut()
    }
    
    /// Updates an existing instance with new data
    #[inline(always)]
    fn update(&mut self, id: K, data: T) -> Option<&mut T> {
        let instance = self.instances.get(&id)?;
        let index = instance.instance_data_index?;
        let instance_data = self.data.get_mut(index)?;

        if instance_data.data.is_none() {
            return None;
        }

        instance_data.data = Some(data);
        instance_data.data.as_mut()
    }

    /// Gets a free slot from the free list or creates a new one
    #[inline(always)]
    fn get_free_slot(&mut self) -> usize {
        // If there's a free index available, use it
        if let Some(index) = self.free_head {
            // Update the free head to the next free index
            self.free_head = self.data[index].next;
            // Clear the next pointer for cleanliness
            self.data[index].next = None;
            // Return the index we're using
            index
        } else {
            // No free indices, add a new one
            let index = self.data.len();
            self.data.push(InstanceData {
                data: None,
                next: None,
            });
            index
        }
    }

    /// Clears all instances and resets the manager
    pub fn clear(&mut self) {
        self.instances.clear();
        self.data.clear();
        self.free_head = None;
    }

    /// Entry API similar to HashMap
    pub fn entry(&mut self, key: K) -> Entry<'_, K, T> {
        if self.contains_key(&key) {
            Entry::Occupied(OccupiedEntry {
                manager: self,
                key,
            })
        } else {
            Entry::Vacant(VacantEntry {
                manager: self,
                key,
            })
        }
    }

    /// Drains all instances from the manager, returning a drain iterator
    pub fn drain(&mut self) -> Drain<'_, K, T> {
        Drain::new(self)
    }

    /// Drains all instances, returning an owned drain iterator
    /// This completely empties the manager and resets it
    pub fn drain_owned(mut self) -> OwnedDrain<K, T> {
        OwnedDrain::new(
            std::mem::take(&mut self.instances),
            std::mem::take(&mut self.data),
        )
    }
}

// Custom iterator for mutable iteration
pub struct IterMut<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    instances: &'a AHashMap<K, Instance<T>>,
    data: &'a mut Vec<InstanceData<T>>,
    keys: std::vec::IntoIter<K>,
}

impl<'a, K, T> Iterator for IterMut<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    type Item = (K, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(key) = self.keys.next() {
            if let Some(instance) = self.instances.get(&key) {
                if let Some(index) = instance.instance_data_index {
                    // Safety: We know this is safe because we're iterating through unique keys
                    // and each key maps to a unique index
                    if let Some(instance_data) = self.data.get_mut(index) {
                        if let Some(data) = instance_data.data.as_mut() {
                            // This is a bit tricky - we need to extend the lifetime
                            // This is safe because we know the data will live as long as the iterator
                            let data_ptr = data as *mut T;
                            return Some((key, unsafe { &mut *data_ptr }));
                        }
                    }
                }
            }
        }
        None
    }
}

// Custom iterator for mutable values
pub struct ValuesMut<'a, T> {
    data: &'a mut Vec<InstanceData<T>>,
    indices: std::vec::IntoIter<usize>,
}

impl<'a, T> Iterator for ValuesMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(index) = self.indices.next() {
            if let Some(instance_data) = self.data.get_mut(index) {
                if let Some(data) = instance_data.data.as_mut() {
                    // Similar lifetime extension as above
                    let data_ptr = data as *mut T;
                    return Some(unsafe { &mut *data_ptr });
                }
            }
        }
        None
    }
}

// Entry API for more ergonomic usage
pub enum Entry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    Occupied(OccupiedEntry<'a, K, T>),
    Vacant(VacantEntry<'a, K, T>),
}

impl<'a, K, T> Entry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn or_insert(self, default: T) -> &'a mut T {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default),
        }
    }

    pub fn or_insert_with<F>(self, default: F) -> &'a mut T
    where
        F: FnOnce() -> T,
    {
        match self {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(default()),
        }
    }

    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut T),
    {
        match self {
            Entry::Occupied(mut entry) => {
                f(entry.get_mut());
                Entry::Occupied(entry)
            }
            Entry::Vacant(entry) => Entry::Vacant(entry),
        }
    }
}

pub struct OccupiedEntry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    manager: &'a mut InstanceManager<K, T>,
    key: K,
}

impl<'a, K, T> OccupiedEntry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn get(&self) -> &T {
        self.manager.get(self.key).expect("Entry key should exist")
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.manager.get_mut(self.key).expect("Entry key should exist")
    }

    pub fn into_mut(self) -> &'a mut T {
        self.manager.get_mut(self.key).expect("Entry key should exist")
    }

    pub fn insert(&mut self, value: T) -> T {
        self.manager.insert(self.key, value).expect("Entry key should exist")
    }

    pub fn remove(self) -> T {
        self.manager.remove(self.key).expect("Entry key should exist")
    }
}

pub struct VacantEntry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    manager: &'a mut InstanceManager<K, T>,
    key: K,
}

impl<'a, K, T> VacantEntry<'a, K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    pub fn insert(self, value: T) -> &'a mut T {
        self.manager.create(self.key, value).expect("Create should succeed for vacant entry")
    }
}

// Implement common traits
impl<K, T> Clone for InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            instances: self.instances.clone(),
            data: self.data.clone(),
            free_head: self.free_head,
        }
    }
}

impl<K, T> PartialEq for InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        
        for key in self.keys() {
            match (self.get(key), other.get(key)) {
                (Some(a), Some(b)) if a == b => continue,
                _ => return false,
            }
        }
        true
    }
}

// FromIterator implementation
impl<K, T> FromIterator<(K, T)> for InstanceManager<K, T>
where
    K: Copy + Clone + PartialEq + Eq + Hash + Send + Sync + Debug + 'static,
{
    fn from_iter<I: IntoIterator<Item = (K, T)>>(iter: I) -> Self {
        let mut manager = Self::new();
        for (key, value) in iter {
            manager.insert(key, value);
        }
        manager
    }
}

// Example usage and tests
#[cfg(test)]
mod tests {
    use super::*;





    #[test]
    fn test_basic_operations() {
        let mut manager = InstanceManager::new();
        
        // Test insertion
        manager.create_or_update(1, "hello".to_string());
        manager.create_or_update(2, "world".to_string());
        
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.get(1), Some(&"hello".to_string()));
        assert_eq!(manager.get(2), Some(&"world".to_string()));
        
        // Test removal
        assert_eq!(manager.remove(1), Some("hello".to_string()));
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.get(1), None);
        
        // Test reuse of freed slot
        manager.create_or_update(3, "rust".to_string());
        assert_eq!(manager.get(3), Some(&"rust".to_string()));
    }

    #[test]
    fn test_entry_api() {
        let mut manager = InstanceManager::new();
        
        // Test vacant entry
        let value = manager.entry(1).or_insert("default".to_string());
        assert_eq!(value, "default");
        
        // Test occupied entry
        let value = manager.entry(1).or_insert("should not overwrite".to_string());
        assert_eq!(value, "default");
    }

    #[test]
    fn test_retain() {
        let mut manager = InstanceManager::new();
        manager.insert(1, 10);
        manager.insert(2, 20);
        manager.insert(3, 30);
        
        // Retain only even values
        manager.retain(|_k, v| *v % 20 == 0);
        
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.get(2), Some(&20));
        assert_eq!(manager.get(1), None);
        assert_eq!(manager.get(3), None);
    }

    #[test]
    fn test_remove_if() {
        let mut manager = InstanceManager::new();
        manager.insert(1, 10);
        manager.insert(2, 20);
        manager.insert(3, 30);
        
        // Remove values greater than 15
        manager.remove_if(|_k, v| *v > 15);
        
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.get(1), Some(&10));
        assert_eq!(manager.get(2), None);
        assert_eq!(manager.get(3), None);
    }

    #[test]
    fn test_for_each_mut() {
        let mut manager = InstanceManager::new();
        manager.insert(1, 10);
        manager.insert(2, 20);
        manager.insert(3, 30);
        
        // Double all values
        manager.for_each_mut(|_k, v| *v *= 2);
        
        assert_eq!(manager.get(1), Some(&20));
        assert_eq!(manager.get(2), Some(&40));
        assert_eq!(manager.get(3), Some(&60));
    }

    #[test]
    fn test_iterators() {
        let mut manager = InstanceManager::new();
        manager.insert(1, 10);
        manager.insert(2, 20);
        
        let sum: i32 = manager.values().sum();
        assert_eq!(sum, 30);
        
        let keys: Vec<_> = manager.keys().collect();
        assert!(keys.contains(&1));
        assert!(keys.contains(&2));
    }
}