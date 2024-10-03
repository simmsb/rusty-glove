use core::fmt::Write;

use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::pipe::{self, Pipe};
use log_log::{Metadata, Record};
use shared::device_to_host::{DeviceToHostMsg, MAX_LOG_LEN};

use crate::messages::distributors::MessageProvenance;
use crate::utils::singleton;
use crate::{messages, sync};

pub fn init() {
    let pipe = singleton!(Pipe<ThreadModeRawMutex, 256>, Pipe::new());
    let (reader, writer) = pipe.split();
    let reader = sync::mutex(reader);
    let logger = singleton!(Logger, Logger { reader, writer });
    let logger = logger as &dyn log_log::Log;
    unsafe {
        let _ = log_log::set_logger_racy(logger)
            .map(|()| log_log::set_max_level_racy(log_log::LevelFilter::Info));
    }
}

struct Logger {
    reader: crate::sync::Mutex<pipe::Reader<'static, ThreadModeRawMutex, 256>>,
    writer: pipe::Writer<'static, ThreadModeRawMutex, 256>,
}

impl log_log::Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut tmp = heapless::String::<128>::new();
            let _ = write!(&mut tmp, "{}\r\n", record.args());
            let src = tmp.as_bytes();

            let _ = self.writer.try_write(src);
        }
        self.flush();
    }

    fn flush(&self) {
        let Some(mut reader) = self.reader.try_lock() else {
            return;
        };
        let Ok(buf) = reader.try_fill_buf() else {
            return;
        };

        let mut emitted = 0;

        for chunk in buf.chunks(MAX_LOG_LEN) {
            let vec = heapless::Vec::from_slice(chunk)
                .expect("Log slice was too big for vec (should be impossible)");

            let cmd = DeviceToHostMsg::Log { msg: vec };

            if messages::try_send_to_host(cmd, MessageProvenance::Origin).is_some() {
                emitted += chunk.len();
            } else {
                reader.consume(emitted);
                return;
            }
        }
        reader.consume(emitted);
    }
}
