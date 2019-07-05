use std::net::{Shutdown, TcpStream};
use std::io;
use std::io::prelude::*;

enum BufOrStream {
    Buf(Vec<u8>),
    Stream(TcpStream),
}

impl Write for BufOrStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            BufOrStream::Buf(b) => b.write(buf),
            BufOrStream::Stream(s) => s.write(buf)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            BufOrStream::Buf(b) => b.flush(),
            BufOrStream::Stream(s) => s.flush()
        }
    }
}

pub struct LogIO {
    log: BufOrStream,
    stream: TcpStream,
    prefix: String
}

impl<'a> LogIO {
    pub fn new(stream: TcpStream, prefix: String) -> Self {
        LogIO {
            log: BufOrStream::Buf(Vec::with_capacity(16 * 1024)),
            stream,
            prefix
        }
    }
    
    pub fn log(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.log.write(self.prefix.as_bytes()) {
            Ok(_) => self.log.write(&buf),
            Err(e) => Err(e)
        }
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Both)
    }

    pub fn switch_log_and_flush(&mut self, mut stream: TcpStream) -> io::Result<()> {
        match &self.log {
            BufOrStream::Buf(existing_buf) => {
                match stream.write(&existing_buf) {
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

impl Write for LogIO {
    fn flush(&mut self) -> io::Result<()> {
        match self.log.flush() {
            Ok(_) => self.stream.flush(),
            Err(e) => Err(e)
        }
    }
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.log.write(&buf) {
            Ok(_) => self.stream.write(&buf),
            Err(e) => Err(e)
        }
    }
}
