use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use std::io;
use std::sync::Arc;

enum BufOrStream<W> {
    Buf(Vec<u8>),
    Stream(Arc<Mutex<W>>),
}

impl<W: AsyncWrite + Unpin> BufOrStream<W> {
    pub async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            BufOrStream::Buf(b) => b.write(buf).await,
            BufOrStream::Stream(s) => { 
                let mut s = s.lock().await;
                s.write(buf).await
            }
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            BufOrStream::Buf(b) => b.write_all(buf).await,
            BufOrStream::Stream(s) => { 
                let mut s = s.lock().await;
                s.write_all(buf).await
            }
        }
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        match self {
            BufOrStream::Buf(b) => b.flush().await,
            BufOrStream::Stream(s) => {
                let mut s = s.lock().await;
                s.flush().await
            }
        }
    }
}

pub struct LogIO<W> {
    log: BufOrStream<W>,
    stream: W,
    prefix: String
}

impl<W: AsyncWrite + Unpin> LogIO<W> {
    pub fn new(stream: W, prefix: String) -> Self {
        LogIO {
            log: BufOrStream::Buf(Vec::with_capacity(16 * 1024)),
            stream,
            prefix
        }
    }
    
    pub async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.log.write(buf).await {
            Ok(_) => self.stream.write(buf).await,
            Err(e) => Err(e)
        }
    }

    pub async fn log(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.log.write(self.prefix.as_bytes()).await {
            Ok(_) => self.log.write(buf).await,
            Err(e) => Err(e)
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self.log.write_all(buf).await {
            Ok(_) => self.stream.write_all(buf).await,
            Err(e) => Err(e)
        }
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        self.stream.flush().await
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown().await
    }

    pub async fn switch_log_and_flush(&mut self, stream: Arc<Mutex<W>>) -> io::Result<()> {
        match &self.log {
            BufOrStream::Buf(existing_buf) => {
                let result: io::Result<usize>;
                {
                    let mut s = stream.lock().await;
                    result = s.write(&existing_buf).await;
                }
                match result {
                    Ok(_) => {
                        self.log = BufOrStream::Stream(stream);
                        Ok(())
                    }
                    Err(e) => Err(e)
                }
            }
            BufOrStream::Stream(_existing_stream) => {
                self.log = BufOrStream::Stream(stream);
                Ok(())
            }
        }
    }
}

