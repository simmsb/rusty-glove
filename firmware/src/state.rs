use core::future::Future;

use crate::sync::{mutex, Mutex};

pub static BLE_ADVERTISING: Mutex<()> = mutex(());

pub async fn with_advertising<T>(f: impl Future<Output = T>) -> T {
    let _l = BLE_ADVERTISING.lock().await;
    f.await
}
