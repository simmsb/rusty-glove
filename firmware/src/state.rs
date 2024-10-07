use core::future::Future;

use crate::sync::{mutex, Mutex, Watch};


pub static USB_CONNECTED: Watch<bool> = Watch::new(false);

pub async fn wait_usb_connected() {
    USB_CONNECTED.wait_for(|c| *c == true).await;
}

pub async fn wait_usb_disconnected() {
    USB_CONNECTED.wait_for(|c| *c == false).await;
}

pub static BLE_ADVERTISING: Mutex<()> = mutex(());

pub async fn with_advertising<T>(f: impl Future<Output = T>) -> T {
    let _l = BLE_ADVERTISING.lock().await;
    f.await
}
