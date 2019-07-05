#![allow(clippy::write_with_newline)]

use std::convert::TryFrom;
use std::env;
use std::thread;
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use std::str;

mod smtp_stream;
use smtp_stream::{SmtpCmd, SmtpStream};

mod log_io;
use log_io::LogIO;

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
    let mail_listener = TcpListener::bind(&smtp_listen).unwrap_or_else(|_| panic!("Unable to bind SMTP listener to {}", &http_listen));
    println!("I'm listening for SMTP on {}", &smtp_listen);

    let emails2 = emails.clone();
    let mail_thread = thread::spawn(move || {
        for stream in mail_listener.incoming() {
            match stream {
                Ok(stream) => {
                    let mail_emails = emails2.clone();
                    thread::spawn(move || {
                        match handle_mail_client(mail_emails.clone(), stream) {
                            Ok(_) => {},
                            Err(e) => eprintln!("problem handling smtp connection: {}", e),
                        }
                    });
                }
                Err(e) => {
                    eprintln!("incoming smtp connection failed: {}", e);
                }
            }
        }
    });

    let web_listener = TcpListener::bind(&http_listen).unwrap_or_else(|_| panic!("Unable to bind HTTP listener to {}", &http_listen));
    println!("I'm listening for HTTP on {}", &http_listen);

    let web_thread = thread::spawn(move || {
        for stream in web_listener.incoming() {
            match stream {
                Ok(stream) => {
                    let web_emails = emails.clone();
                    thread::spawn(move || {
                        match handle_web_client(web_emails.clone(), stream) {
                            Ok(_) => {},
                            Err(e) => eprintln!("problem handling http connection: {}", e),
                        }
                    });
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

    s.to_lowercase()
}

struct Email {
    user: String,
    #[allow(dead_code)]
    domain: String
}

impl TryFrom<&str> for Email {
    type Error = &'static str;

    fn try_from(smtp_email: &str) -> Result<Self, Self::Error> {
        let mut user = String::new();
        let mut domain = String::new();

        let mut chars = smtp_email.chars();
        
        let c = chars.next().unwrap_or('\0');
        if c == '\0' {
            return Err("First char was empty, unable to make Email");
        }

        if c != '<' {
            user.push(c);
        }

        for c in &mut chars {
            if c == '@' { break; }
            user.push(c);
        }

        for c in &mut chars {
            if c == '>' { break; }
            domain.push(c);
        }

        if user.is_empty() || domain.is_empty() {
            Err("Unable to parse user or domain for Email")
        } else {
            Ok(Email{user, domain})
        }
    }
}

fn handle_web_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {
    let mut stream_writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    if let Some(line) = lines.next() {
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

            match stream_writer.shutdown(Shutdown::Both) {
                Ok(_) => {},
                Err(e) => {
                    if e.kind() != io::ErrorKind::NotConnected {
                        eprintln!("problem shutting down connection: {}", e)
                    }
                }
            }

            {
                let mut hsh = emails.lock().unwrap();
                let _res = hsh.remove(&user_name);
            }

        } else {
            write!(stream_writer, "HTTP/1.1 405 Method Not Allowed\r\n")?;
        }
    }
    Ok(())
}

fn handle_mail_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {

    let mut stream_writer = stream.try_clone()?;
    write!(stream_writer, "220 emailpipe.sh SMTP emailpipe\r\n")?;

    let mut w = LogIO::new(stream_writer, String::from(">> "));
    let smtp_stream = SmtpStream::new(stream);

    for wrapped_cmd in smtp_stream {
        if let Err(e) = w.log(wrapped_cmd.raw.as_bytes()) {
            return Err(e)
        }

        match wrapped_cmd.cmd {
            SmtpCmd::Helo(_domain) => {
                write!(w, "250 emailpipe.sh, welcome\r\n")?;
            }
            SmtpCmd::RcptTo(raw_email) => {
                match Email::try_from(raw_email.as_str()) {
                    Err(_) => write!(w, "501 bad syntax\r\n")?,
                    Ok(email) => {
                        let hsh = emails.lock().unwrap();
                        let http_stream = hsh.get(&email.user);

                        match http_stream {
                            None => write!(w, "550 no such user\r\n")?,
                            Some(user_stream) => {
                                let s = (*user_stream).try_clone()?;
                                if let Err(e) = w.switch_log_and_flush(s) {
                                    return Err(e);
                                }
                                write!(w, "250 ok\r\n")?;
                            }
                        }
                    }
                }
            }
            SmtpCmd::MailFrom(raw_email) => {
                match Email::try_from(raw_email.as_str()) {
                    Err(_) => write!(w, "501 bad syntax\r\n")?,
                    Ok(_email) => write!(w, "250 ok\r\n")?
                }
            }
            SmtpCmd::DataStart => {
                write!(w, "354 End data with <CR><LF>.<CR><LF>\r\n")?;
            }
            SmtpCmd::DataPart(_data) => {
                
            }
            SmtpCmd::DataEnd => {
                write!(w, "250 Ok: queued message\r\n")?;
            }
            SmtpCmd::Quit => {
                write!(w, "221 bye\r\n")?;
                if let Err(e) = w.flush() {
                    eprintln!("Error flushing logio during quit: {}", e);
                }
                if let Err(e) = w.shutdown() {
                    eprintln!("Error shuting down logio stream during quit: {}", e);
                }
                break;
            }
            SmtpCmd::Unknown(_line) => {
                write!(w, "500 what?\r\n")?;
            }
        }
    }

    Ok(())
}

