#![allow(clippy::write_with_newline)]

use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex;
use std::convert::TryFrom;
use std::env;
use std::sync::Arc;
use std::collections::HashMap;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use std::str;

mod smtp_stream;
use smtp_stream::{SmtpCmd, SmtpStream};

mod log_io;
use log_io::LogIO;

type W = Arc<Mutex<OwnedWriteHalf>>;
type EmailStore = Arc<Mutex<HashMap<String, W>>>;

#[tokio::main]
async fn main() -> io::Result<()> {

    let email_connections: HashMap<String, W> = HashMap::new();
    let emails: EmailStore = Arc::new(Mutex::new(email_connections));
    let http_listen = match env::var("HTTP_LISTEN") {
        Ok(val) => val,
        Err(_e) => "127.0.0.1:9001".to_string()
    };
    let smtp_listen = match env::var("SMTP_LISTEN") {
        Ok(val) => val,
        Err(_e) => "127.0.0.1:9002".to_string()
    };
    let mail_listener = TcpListener::bind(&smtp_listen).await.unwrap_or_else(|_| panic!("Unable to bind SMTP listener to {}", &smtp_listen));
    let web_listener = TcpListener::bind(&http_listen).await.unwrap_or_else(|_| panic!("Unable to bind HTTP listener to {}", &http_listen));

    println!("I'm listening for SMTP on {}", &smtp_listen);
    println!("I'm listening for HTTP on {}", &http_listen);

    loop {
        tokio::select! {
            r = mail_listener.accept() => {
                eprintln!("got mail connection");
                match r {
                    Ok((stream, _addr)) => {
                        let mails_emails = emails.clone();
                        tokio::spawn(async move {
                            match handle_mail_client(mails_emails, stream).await {
                                Ok(_) => {},
                                Err(e) => eprintln!("problem handling smtp connection: {}", e),
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("incoming smtp connection failed: {}", e);
                    }
                }
            },
            r = web_listener.accept() => {
                eprintln!("got web connection");
                match r {
                    Ok((stream, _addr)) => {
                        let web_emails = emails.clone();
                        tokio::spawn(async move {
                            match handle_web_client(web_emails, stream).await {
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
        };
    }

    Ok(())
}

fn rand_user() -> String {
    let rng = thread_rng();
    let mut s = String::with_capacity(9);
    let mut riter = rng.sample_iter(&Alphanumeric);
    for _ in 0..4 {
        s.push(riter.next().unwrap() as char);
    }

    s.push('.');

    for _ in 0..4 {
        s.push(riter.next().unwrap() as char);
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

async fn handle_web_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {
    let (reader, mut writer) = stream.into_split();

    let reader = tokio::io::BufReader::new(reader);

    let mut lines = reader.lines();

    if let Some(line) = lines.next_line().await? {
        if line.starts_with("GET / HTTP") {
            writer.write_all("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n".as_bytes()).await?;
            writer.write_all("USAGE\n\n".as_bytes()).await?;
            writer.write_all("curl emailpipe.sh/listen\r\n".as_bytes()).await?;
        } else if line.starts_with("GET /listen HTTP") {
            writer.write_all("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n".as_bytes()).await?;

            let user_name = rand_user();

            writer.write_all("Listening for mail at ".as_bytes()).await?;
            writer.write_all(user_name.as_bytes()).await?;
            writer.write_all("@emailpipe.sh\n".as_bytes()).await?;

            let writer = Arc::new(Mutex::new(writer));

            {
                let mut hsh = emails.lock().await;
                let res = hsh.insert(user_name.clone(), writer.clone());

                if let Some(old_stream) = res {
                    let mut old_stream = old_stream.lock().await;
                    old_stream.write_all("closed by another connection".as_bytes()).await?;
                }
            }

            let mut nline = lines.next_line().await?;

            while nline.is_some() {
                // nothing
                nline = lines.next_line().await?;
            }

            {
                let mut writer = writer.lock().await;
                match writer.shutdown().await {
                    Ok(_) => {},
                    Err(e) => {
                        if e.kind() != io::ErrorKind::NotConnected {
                            eprintln!("problem shutting down connection: {}", e)
                        }
                    }
                }
            }

            {
                let mut hsh = emails.lock().await;
                let _res = hsh.remove(&user_name);
            }

        } else {
            writer.write_all("HTTP/1.1 405 Method Not Allowed\r\n".as_bytes()).await?;
        }
    }
    Ok(())
}

async fn handle_mail_client(emails: EmailStore, stream: TcpStream) -> io::Result<()> {

    let (reader, writer) = stream.into_split();
    let mut w = LogIO::new(writer, String::from(">> "));

    w.write_all("220 emailpipe.sh SMTP emailpipe\r\n".as_bytes()).await?;
    let mut smtp_stream = SmtpStream::new(reader);

    while let Some(wrapped_cmd) = smtp_stream.next().await {
        if let Err(e) = w.log(wrapped_cmd.raw.as_bytes()).await {
            return Err(e)
        }

        match wrapped_cmd.cmd {
            SmtpCmd::Helo(_domain) => {
                w.write_all("250 emailpipe.sh, welcome\r\n".as_bytes()).await?;
            }
            SmtpCmd::RcptTo(raw_email) => {
                match Email::try_from(raw_email.as_str()) {
                    Err(_) => w.write_all("501 bad syntax\r\n".as_bytes()).await?,
                    Ok(email) => {
                        let hsh = emails.lock().await;
                        let http_stream = hsh.get(&email.user);

                        match http_stream {
                            None => w.write_all("550 no such user\r\n".as_bytes()).await?,
                            Some(user_stream) => {
                                let s = user_stream.clone();
                                if let Err(e) = w.switch_log_and_flush(s).await {
                                    return Err(e);
                                }
                                w.write_all("250 ok\r\n".as_bytes()).await?
                            }
                        }
                    }
                };
            }
            SmtpCmd::MailFrom(raw_email) => {
                match Email::try_from(raw_email.as_str()) {
                    Err(_) => w.write_all("501 bad syntax\r\n".as_bytes()).await?,
                    Ok(_email) => w.write_all("250 ok\r\n".as_bytes()).await?
                };
            }
            SmtpCmd::DataStart => {
                w.write_all("354 End data with <CR><LF>.<CR><LF>\r\n".as_bytes()).await?;
            }
            SmtpCmd::DataPart(_data) => {
                
            }
            SmtpCmd::DataEnd => {
                w.write_all("250 Ok: queued message\r\n".as_bytes()).await?;
            }
            SmtpCmd::Quit => {
                w.write_all("221 bye\r\n".as_bytes()).await?;
                if let Err(e) = w.flush().await {
                    eprintln!("Error flushing logio during quit: {}", e);
                }
                if let Err(e) = w.shutdown().await {
                    eprintln!("Error shuting down logio stream during quit: {}", e);
                }
                break;
            }
            SmtpCmd::Unknown(_line) => {
                w.write_all("500 what?\r\n".as_bytes()).await?;
            }
        }
    }

    Ok(())
}

