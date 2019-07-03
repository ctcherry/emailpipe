use std::env;
use std::fmt;
use std::thread;
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use std::str;

type EmailStore = Arc<Mutex<HashMap<String, TcpStream>>>;

fn main() -> io::Result<()> {

    let email_connections: HashMap<String, TcpStream> = HashMap::new();
    let emails = Arc::new(Mutex::new(email_connections));
    let http_listen = match env::var("HTTP_LISTEN") {
        Ok(val) => val,
        Err(_e) => "127.0.0.1:9001".to_string()
    };
    let smtp_listen = match env::var("SMTP_LISTEN") {
        Ok(val) => val,
        Err(_e) => "127.0.0.1:9002".to_string()
    };
    let mail_listener = TcpListener::bind(&smtp_listen).expect(format!("Unable to bind SMTP listener to {}", &smtp_listen).as_ref());
    println!("I'm listening for SMTP on {}", &smtp_listen);

    let mail_emails = emails.clone();
    let mail_thread = thread::spawn(move || {
        for stream in mail_listener.incoming() {
            match stream {
                Ok(stream) => {
                    handle_mail_client(mail_emails.clone(), stream);
                }
                Err(e) => {
                    eprintln!("incoming smtp connection failed: {}", e);
                }
            }
        }
    });

    let web_listener = TcpListener::bind(&http_listen).expect(format!("Unable to bind HTTP listener to {}", &http_listen).as_ref());
    println!("I'm listening for HTTP on {}", &http_listen);

    let web_emails = emails.clone();
    let web_thread = thread::spawn(move || {
        for stream in web_listener.incoming() {
            match stream {
                Ok(stream) => {
                    handle_web_client(web_emails.clone(), stream);
                }
                Err(e) => {
                    eprintln!("incoming http connection failed: {}", e);
                }
            }
        }
    });

    mail_thread.join().unwrap();
    web_thread.join().unwrap();
    Ok(())
}

fn rand_user() -> String {
    let mut rng = thread_rng();
    let mut s = String::with_capacity(9);
    let mut riter = rng.sample_iter(&Alphanumeric);
    for _ in 0..4 {
        s.push(riter.next().unwrap());
    }

    s.push('.');

    for _ in 0..4 {
        s.push(riter.next().unwrap());
    }

    return s.to_lowercase();
}

struct Email {
    user: String,
    domain: String
}

fn parse_email(smtp_email: &str) -> Option<Email> {
    let mut user = String::new();
    let mut domain = String::new();

    let mut chars = smtp_email.chars();
    
    let c = chars.next()?;
    if c != '<' {
        user.push(c);
    }

    while let Some(c) = chars.next() {
        if c == '@' { break; }
        user.push(c);
    }

    while let Some(c) = chars.next() {
        if c == '>' { break; }
        domain.push(c);
    }
    
    return Some(Email{user: user, domain: domain});
}

fn handle_web_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {
    let mut stream_writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    let mut user: Option<String> = None;
    match lines.next() {
        Some(line) => {
            let sline = line.unwrap();
            if sline.starts_with("GET / HTTP") {
                write!(stream_writer, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n")?;
                write!(stream_writer, "USAGE\n\n")?;
                write!(stream_writer, "curl emailpipe.sh/listen\r\n")?;
            } else if sline.starts_with("GET /listen HTTP") {
                write!(stream_writer, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n")?;

                let user_name = rand_user();

                {
                    let mut hsh = emails.lock().unwrap();
                    let res = hsh.insert(user_name.clone(), stream_writer.try_clone()?);

                    if let Some(mut old_stream) = res {
                       write!(old_stream, "closed by another connection")?;
                    }
                }


                write!(stream_writer, "Listening for mail at {}@emailpipe.sh\n", &user_name)?;
                let mut nline = lines.next();
                while let Some(_) = nline {
                    // nothing
                    nline = lines.next();
                }

                stream_writer.shutdown(Shutdown::Both);

                {
                    let mut hsh = emails.lock().unwrap();
                    let _res = hsh.remove(&user_name);
                }

            } else {
                write!(stream_writer, "HTTP/1.1 405 Method Not Allowed\r\n")?;
            }
        }
        None => {}
    }
    Ok(())
}

enum SMTPCmd {
    Helo(String),
    RcptTo(String),
    MailFrom(String),
    DataStart,
    DataPart(String),
    DataEnd,
    Quit,
    Unknown(String)
}

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

struct LogIO {
    log: BufOrStream,
    stream: TcpStream
}

impl Write for LogIO {
    fn flush(&mut self) -> io::Result<()> {
        self.log.flush();
        self.stream.flush()
    }
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.log.write(&buf);
        self.stream.write(&buf)
    }
}

impl<'a> LogIO {
    fn new(stream: TcpStream) -> Self {
        LogIO {
            log: BufOrStream::Buf(Vec::with_capacity(16 * 1024)),
            stream
        }
    }
    
    fn log(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.log.write(&buf)
    }

    fn shutdown(&mut self) {
        self.stream.shutdown(Shutdown::Both);
        match &self.log {
            BufOrStream::Buf(_) => {}
            BufOrStream::Stream(existing_stream) => {
                // existing_stream.shutdown(Shutdown::Both);
            }
        }
    }

    fn switch_log_and_flush(&mut self, mut stream: TcpStream) {
        match &self.log {
            BufOrStream::Buf(existing_buf) => {
                stream.write(&existing_buf);
                self.log = BufOrStream::Stream(stream);
            }
            BufOrStream::Stream(_existing_stream) => {
                self.log = BufOrStream::Stream(stream);
            }
        }
    }
}

struct SmtpStream {
    line_buf: String,
    reader: io::BufReader<TcpStream>,
    capturing_data: bool
}

impl SmtpStream {
    fn new(stream: TcpStream) -> Self {
        SmtpStream {
            line_buf: String::with_capacity(1024),
            reader: BufReader::new(stream),
            capturing_data: false
        }
    }
}

impl Iterator for SmtpStream {

    type Item = SMTPCmd;

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
                        return Some(SMTPCmd::DataEnd)
                    }
                    let val = self.line_buf.clone();
                    return Some(SMTPCmd::DataPart(val))
                }

                let mut cmd_buf = self.line_buf.clone();
                cmd_buf.make_ascii_lowercase();
                let cmd = match true {
                    _ if cmd_buf.starts_with("helo") => {
                        // 5 = len(helo) + 1
                        let value_start = 5;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        return Some(SMTPCmd::Helo(val))
                    },
                    _ if cmd_buf.starts_with("rcpt to") => {
                        // 8 = len(rcpt to) + 1
                        let value_start = 8;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        return Some(SMTPCmd::RcptTo(val))
                    },
                    _ if cmd_buf.starts_with("mail from") => {
                        // 10 = len(mail from) + 1
                        let value_start = 10;
                        let value_end = line_len - 2;
                        let val = String::from(&self.line_buf[value_start..value_end]);
                        return Some(SMTPCmd::MailFrom(val))
                    },
                    _ if cmd_buf.starts_with("data") => {
                        if cmd_buf == "data\r\n" {
                            self.capturing_data = true;
                            return Some(SMTPCmd::DataStart)
                        }
                    },
                    _ if cmd_buf.starts_with("quit") => {
                        return Some(SMTPCmd::Quit)
                    }
                };

                let val = self.line_buf.clone();
                return Some(SMTPCmd::Unknown(val))
            }
            Err(err) => {
                return None
            }
        }

    }

}

fn handle_mail_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {

    let smtp_stream = SmtpStream::new(stream);

    for cmd in smtp_stream {
        match cmd {
            SMTPCmd::Helo(_domain) => {
                write!(w, "250 emailpipe.sh, welcome\r\n")?;
            }
            SMTPCmd::RcptTo(raw_email) => {

                match parse_email(raw_email) {
                    None => write!(w, "501 bad syntax\r\n")?,
                    Some(email) => {
                        let hsh = emails.lock().unwrap();
                        let http_stream = hsh.get(&email.user);

                        match http_stream {
                            None => write!(w, "550 no such user\r\n")?,
                            Some(user_stream) => {
                                let s = (*user_stream).try_clone()?;
                                w.switch_log_and_flush(s);
                                write!(w, "250 ok\r\n")?;
                            }
                        }
                    }
                }
            }
            SMTPCmd::MailFrom(raw_email) => {
                match parse_email(raw_email) {
                    None => write!(w, "501 bad syntax\r\n")?,
                    Some(_email) => write!(w, "250 ok\r\n")?
                }
            }
            SMTPCmd::DataStart => {
                write!(w, "354 End data with <CR><LF>.<CR><LF>\r\n")?;
                capture_data = true;
            }
            SMTPCmd::DataPart(_data) => {
                write!(w, "250 Ok: queued message\r\n")?;
                capture_data = false;
            }
            SMTPCmd::DataEnd => {
                write!(w, "250 Ok: queued message\r\n")?;
                capture_data = false;
            }
            SMTPCmd::Quit => {
                write!(w, "221 bye\r\n")?;
                w.shutdown();
                break;
            }
            SMTPCmd::Unknown(_) => {
                write!(w, "500 what?\r\n")?;
            }
        }
    }

    Ok(())
}

fn handle_mail_client2(emails: EmailStore, stream: TcpStream) -> io::Result<()> {
    let mut stream_writer = stream.try_clone()?;
    write!(stream_writer, "220 emailpipe.sh SMTP emailpipe\r\n")?;
    let mut reader = BufReader::new(stream);
    let mut line_buf = String::with_capacity(256);
    let mut capture_data = false;
    let mut data_buf = Vec::with_capacity(2048);
    let mut w = LogIO::new(stream_writer);

    while let Ok(line_len) = reader.read_line(&mut line_buf) {
        w.log(line_buf.as_bytes())?;
        let mut cmd_buf = line_buf.clone();
        cmd_buf.make_ascii_lowercase();
        line_buf.clear();


        let cmd = match true {
            _ if capture_data => {
                data_buf.clear();
                while let Ok(_) = reader.read_line(&mut line_buf) {
                    w.log(line_buf.as_bytes())?;
                    write!(data_buf, "{}", line_buf)?;
                    if line_buf == ".\r\n" {
                        break;
                    }
                    line_buf.clear();
                }
                line_buf.clear();
                let val = str::from_utf8(&data_buf).unwrap();
                SMTPCmd::Data(val)
            },
            _ if cmd_buf.starts_with("helo") => {
                // 5 = len(helo) + 1
                let value_start = 5;
                let value_end = line_len - 2;
                let val = str::from_utf8(&cmd_buf.as_bytes()[value_start..value_end]).unwrap();
                SMTPCmd::Helo(val)
            },
            _ if cmd_buf.starts_with("rcpt to") => {
                // 8 = len(rcpt to) + 1
                let value_start = 8;
                let value_end = line_len - 2;
                let val = str::from_utf8(&cmd_buf.as_bytes()[value_start..value_end]).unwrap();
                SMTPCmd::RcptTo(val)
            },
            _ if cmd_buf.starts_with("mail from") => {
                // 10 = len(mail from) + 1
                let value_start = 10;
                let value_end = line_len - 2;
                let val = str::from_utf8(&cmd_buf.as_bytes()[value_start..value_end]).unwrap();
                SMTPCmd::MailFrom(val)
            },
            _ if cmd_buf.starts_with("data") => {
                if cmd_buf == "data\r\n" {
                    capture_data = true;
                    SMTPCmd::StartData
                } else {
                    let val = str::from_utf8(&cmd_buf.as_bytes()).unwrap();
                    SMTPCmd::Unknown(val)
                }
            },
            _ if cmd_buf.starts_with("quit") => {
                SMTPCmd::Quit
            },
            _ => {
                let val = str::from_utf8(&cmd_buf.as_bytes()).unwrap();
                SMTPCmd::Unknown(val)
            }
        };

        match cmd {
            SMTPCmd::Helo(_domain) => {
                write!(w, "250 emailpipe.sh, welcome\r\n")?;
            }
            SMTPCmd::RcptTo(raw_email) => {

                match parse_email(raw_email) {
                    None => write!(w, "501 bad syntax\r\n")?,
                    Some(email) => {
                        let hsh = emails.lock().unwrap();
                        let http_stream = hsh.get(&email.user);

                        match http_stream {
                            None => write!(w, "550 no such user\r\n")?,
                            Some(user_stream) => {
                                let s = (*user_stream).try_clone()?;
                                w.switch_log_and_flush(s);
                                write!(w, "250 ok\r\n")?;
                            }
                        }
                    }
                }
            }
            SMTPCmd::MailFrom(raw_email) => {
                match parse_email(raw_email) {
                    None => write!(w, "501 bad syntax\r\n")?,
                    Some(_email) => write!(w, "250 ok\r\n")?
                }
            }
            SMTPCmd::StartData => {
                write!(w, "354 End data with <CR><LF>.<CR><LF>\r\n")?;
                capture_data = true;
            }
            SMTPCmd::Data(_data) => {
                write!(w, "250 Ok: queued message\r\n")?;
                capture_data = false;
            }
            SMTPCmd::Quit => {
                write!(w, "221 bye\r\n")?;
                w.shutdown();
                break;
            }
            SMTPCmd::Unknown(_) => {
                write!(w, "500 what?\r\n")?;
            }
        }
    }

    Ok(())
}
