#[macro_export]
macro_rules! inner_or_clone_arcmutex {
    ($arc:block) => {
        match Arc::try_unwrap($arc) {
            Ok(mutex) => mutex.into_inner().unwrap(), // Safe because it is the only reference to the mutex
            Err(arcmutex) => arcmutex.lock().unwrap().clone(),
        };

    }
}
