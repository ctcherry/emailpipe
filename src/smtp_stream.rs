use std::net::{TcpStream};
use std::io::{BufReader, BufRead};

pub struct SmtpCmdWrapper {
    pub cmd: SmtpCmd,
    pub raw: String
}

fn cmd(cmd: SmtpCmd, raw: String) -> SmtpCmdWrapper {
    SmtpCmdWrapper {
        cmd,
        raw
    }
}

pub enum SmtpCmd {
    Helo(String),
    RcptTo(String),
    MailFrom(String),
    DataStart,
    DataPart(String),
    DataEnd,
    Quit,
    Unknown(String)
}

pub struct SmtpStream {
    line_buf: String,
    reader: BufReader<TcpStream>,
    capturing_data: bool
}

impl SmtpStream {
    pub fn new(stream: TcpStream) -> Self {
        SmtpStream {
            line_buf: String::with_capacity(1024),
            reader: BufReader::new(stream),
            capturing_data: false
        }
    }
}

impl Iterator for SmtpStream {

    type Item = SmtpCmdWrapper;

    // next() is the only required method
    fn next(&mut self) -> Option<Self::Item> {

        self.line_buf.clear();

        match self.reader.read_line(&mut self.line_buf) {
            Ok(line_len) => {
                if line_len == 0 {
                    return None;
                }
                if self.capturing_data {
                    if self.line_buf == ".\r\n" {
                        self.capturing_data = false;
                        return Some(cmd(SmtpCmd::DataEnd, self.line_buf.clone()))
                    }
                    let val = self.line_buf.clone();
                    return Some(cmd(SmtpCmd::DataPart(val.clone()), val))
                }

                let mut cmd_buf = self.line_buf.clone();
                cmd_buf.make_ascii_lowercase();
                match () {
                    _ if cmd_buf.starts_with("helo") => {
                        // 5 = len(helo) + 1
                        let value_start = 5;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        Some(cmd(SmtpCmd::Helo(val), self.line_buf.clone()))
                    },
                    _ if cmd_buf.starts_with("rcpt to") => {
                        // 8 = len(rcpt to) + 1
                        let value_start = 8;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        Some(cmd(SmtpCmd::RcptTo(val), self.line_buf.clone()))
                    },
                    _ if cmd_buf.starts_with("mail from") => {
                        // 10 = len(mail from) + 1
                        let value_start = 10;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        Some(cmd(SmtpCmd::MailFrom(val), self.line_buf.clone()))
                    },
                    _ if cmd_buf == "data\r\n" => {
                        self.capturing_data = true;
                        Some(cmd(SmtpCmd::DataStart, self.line_buf.clone()))
                    },
                    _ if cmd_buf.starts_with("quit") => {
                        Some(cmd(SmtpCmd::Quit, self.line_buf.clone()))
                    },
                    _ => {
                        let val = self.line_buf.clone();
                        Some(cmd(SmtpCmd::Unknown(val.clone()), val))
                    }
                }
            }
            Err(_) => None
        }

    }

}
