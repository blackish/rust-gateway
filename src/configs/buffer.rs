use bytes::BufMut;
use std::io;
use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use std::task::{self, Waker};
use std::sync::Mutex;

#[derive(Debug)]
pub struct StrictBuffer {
    buffer: Vec<u8>,
    is_shutdown: bool,
    read_cursor: usize,
    write_cursor: usize,
    waker: Option<task::Waker>
}

impl StrictBuffer {
    pub fn new(size: usize) -> (StrictBufferWriter, StrictBufferReader) {
        let buff = Arc::new(Mutex::new(Self {
            buffer: vec![0; size],
            is_shutdown: false,
            read_cursor: 0,
            write_cursor: 0,
            waker: None
        }));
        let reader = StrictBufferReader{buffer: Arc::clone(&buff)};
        let writer = StrictBufferWriter{buffer: Arc::clone(&buff)};
        return (writer, reader)
    }

    fn park(&mut self, waker: &Waker) {
        self.waker = Some(waker.clone());
    }

    fn wake(&mut self) {
        if let Some(w) = self.waker.take() {
            w.wake()
        }
    }
}

#[derive(Debug)]
pub struct StrictBufferReader {
    buffer: Arc<Mutex<StrictBuffer>>
}

#[derive(Debug)]
pub struct StrictBufferWriter {
    buffer: Arc<Mutex<StrictBuffer>>
}

impl AsyncWrite for StrictBufferWriter {
    fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut task::Context<'_>,
            buf: &[u8],
        ) -> task::Poll<Result<usize, io::Error>> {
        let mut read_buf = self.buffer.lock().unwrap();
        if read_buf.is_shutdown {
            return task::Poll::Ready(Ok(0))
        }
        let write_cursor = read_buf.write_cursor;
        if write_cursor < read_buf.buffer.len() {
            let result = read_buf.buffer[write_cursor..]
                            .writer()
                            .write(buf)
                            .unwrap();
            read_buf.write_cursor += result;
            // println!("{:?} {:?} {:?}", read_buf.write_cursor, buf, read_buf.buffer);
            read_buf.wake();
            return task::Poll::Ready(Ok(result))
        }
        read_buf.park(cx.waker());
        return task::Poll::Pending;
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut task::Context<'_>) -> task::Poll<Result<(), io::Error>> {
        task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: std::pin::Pin<&mut Self>, _cx: &mut task::Context<'_>) -> task::Poll<Result<(), io::Error>> {
        let mut read_buf = self.buffer.lock().unwrap();
        if !read_buf.is_shutdown {
            read_buf.wake();
        }
        read_buf.is_shutdown = true;
        task::Poll::Ready(Ok(()))
    }
}

impl AsyncRead for StrictBufferReader {
    fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> task::Poll<io::Result<()>> {
        let mut read_buf = self.buffer.lock().unwrap();
        if read_buf.is_shutdown || (read_buf.read_cursor >= read_buf.write_cursor) {
            if Arc::strong_count(&self.buffer) == 1 {
                return task::Poll::Ready(Ok(()))
            }
            read_buf.park(cx.waker());
            return task::Poll::Pending;
        }
        let result = buf
            .writer()
            .write(&read_buf.buffer[read_buf.read_cursor..read_buf.write_cursor])?;
        read_buf.read_cursor += result;
        if read_buf.read_cursor >= read_buf.write_cursor {
            read_buf.read_cursor = 0;
            read_buf.write_cursor = 0;
        }
        read_buf.wake();
        task::Poll::Ready(Ok(()))
    }
}



pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;
    use tokio::time::sleep;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_ring_buffer() {
        println!("Spawning tasks");
        let (writer, reader) = StrictBuffer::new(1024);
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        let a = tokio::spawn(async move{
            let mut buf: Vec<u8> = vec![0;1024];
            // println!("{:?}", reader);
            let mut rd: StrictBufferReader = rx1.await.unwrap();
            let res = rd.read(&mut buf).await.unwrap();
            println!("{:?}", &buf[..res]);
        });
        let _ = tx1.send(reader);
        let b = tokio::spawn(async move{
            // println!("{:?}", writer);
            let mut wr: StrictBufferWriter = rx2.await.unwrap();
            sleep(Duration::from_secs(10)).await;
            let res = wr.write(&b"12345"[..]).await.unwrap();
            println!("Done writing: {:?}", res);
        });
        let _  = tx2.send(writer);
        let _ = a.await;
        let _ = b.await;
    }
}
