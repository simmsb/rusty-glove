//! postcard messages over a byte stream, delimited with COBS

use embedded_io_async::{Read, Write};
use postcard::accumulator::{CobsAccumulator, FeedResult};
use serde::{de::DeserializeOwned, Serialize};

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadError<E> {
    Io(E),
    /// The other end closed the connection
    Eof,
}

/// Send one message, `N` must fit the COBS encoded message
pub async fn write_msg<const N: usize, W: Write, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> Result<(), W::Error> {
    let mut buf = [0u8; N];
    let frame = postcard::to_slice_cobs(msg, &mut buf).expect("message too large for frame");
    w.write_all(frame).await?;
    w.flush().await
}

/// Reads messages off a stream, `N` must fit the largest COBS encoded message
pub struct MsgReader<const N: usize> {
    acc: CobsAccumulator<N>,
    buf: [u8; N],
    start: usize,
    end: usize,
}

impl<const N: usize> Default for MsgReader<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MsgReader<N> {
    pub const fn new() -> Self {
        Self {
            acc: CobsAccumulator::new(),
            buf: [0; N],
            start: 0,
            end: 0,
        }
    }

    /// Read the next message, skipping any that don't decode
    pub async fn read<R: Read, T: DeserializeOwned>(
        &mut self,
        r: &mut R,
    ) -> Result<T, ReadError<R::Error>> {
        loop {
            if self.start == self.end {
                let n = r.read(&mut self.buf).await.map_err(ReadError::Io)?;
                if n == 0 {
                    return Err(ReadError::Eof);
                }
                self.start = 0;
                self.end = n;
            }

            let window = &self.buf[self.start..self.end];
            let (msg, remaining) = match self.acc.feed::<T>(window) {
                FeedResult::Consumed => (None, 0),
                FeedResult::OverFull(rest) | FeedResult::DeserError(rest) => (None, rest.len()),
                FeedResult::Success { data, remaining } => (Some(data), remaining.len()),
            };
            self.start = self.end - remaining;

            if let Some(msg) = msg {
                return Ok(msg);
            }
        }
    }
}
