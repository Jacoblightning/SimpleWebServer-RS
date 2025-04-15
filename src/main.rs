use std::collections::HashMap;
use colored::Colorize;
use regex::Regex;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::path::{PathBuf, absolute};
use chrono::{DateTime, Duration, Timelike, Utc};

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    // Bind IP Address
    #[arg(default_value = "127.0.0.1")]
    bindto: String,
    // Bind Port
    #[arg(default_value_t = 8080)]
    port: u16,
    #[arg(long, default_value_t = false, help="Disable logging. Primary use case is for company's who say they don't store logs.")]
    zerologs: bool,
    #[arg(short = 'r', long, default_value_t = 120, help="Maximum requests per second before rate-limiting. 0 to disable")]
    ratelimit: u16,
    #[arg(short = 'd', long, default_value_t = 180, help="Timeout in seconds after exceeding ratelimit")]
    timeout: i64,
    #[arg(short = 'v', long, default_value_t = false, help="Use verbose output")]
    verbose: bool,
}

fn error_stream(mut stream: TcpStream, error_id: u16) {
    stream
        .write(format!("HTTP/1.1 {} Bad Request\n\n{}\n", error_id, error_id).as_bytes())
        .unwrap();
    stream.flush().unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
}

fn print_message(ip: String, path: &str, error_id: u16) {
    let message = format!("{}: GET {} - {}", ip, path, error_id);
    if error_id == 200 {
        println!("{}", message.green());
    } else {
        println!("{}", message.yellow());
    }
}

fn handle_client(mut stream: TcpStream, header_regex: &Regex, zlog: bool) {
    let peer = stream.peer_addr().unwrap();
    //println!("Connection from {}", peer.to_string());
    
    let mut buffer: [u8; 4096] = [0; 4096];
    stream.read(&mut buffer).unwrap();

    let header = String::from_utf8_lossy(&buffer);

    if !header_regex.is_match(&header) {
        if !zlog {
            println!("{}", "GET - 400".yellow());
        }
        error_stream(stream, 400);
        return;
    }

    let m = header_regex.captures(&header).unwrap();

    // Path parsing
    let mut path: PathBuf = absolute(PathBuf::from(&m[1])).unwrap();

    if path == PathBuf::from("/") {
        // If requesting root, change to index.html
        path.push("index.html");
    }

    /*
    if path.extension() == None {
        // Add .html to non html paths
        path.set_extension("html");
    }
     */

    // Convert into a relative path
    path = PathBuf::from(path.strip_prefix("/").unwrap());

    if !path.exists() {
        if !zlog {
            print_message(peer.ip().to_string(), &m[1], 404);
        }
        error_stream(stream, 404);
        return;
    }

    match path.canonicalize() {
        Ok(_path) => {
            path = _path;
        }
        Err(_) => {
            if !zlog {
                println!(
                    "{}",
                    format!("!!! TOCTOU Prevented: {} !!!", path.display()).red()
                );
            }
            error_stream(stream, 404);
            return;
        }
    }

    // Protection from directory escape
    if !path.starts_with(PathBuf::from(".").canonicalize().unwrap()) {
        if !zlog {
            println!(
                "{}",
                format!("!!! Directory escape prevented: {} !!!", path.display()).red()
            );
        }
        error_stream(stream, 404);
        return;
    }

    let file = fs::read(path);

    match file {
        Ok(file) => {
            if !zlog {
                print_message(peer.ip().to_string(), &m[1], 200);
            }
            stream.write_all(b"HTTP/1.1 200 OK\n\n").unwrap();
            stream.write_all(&*file).unwrap();
        }
        Err(e) => {
            if !zlog {
                println!(
                    "{}",
                    format!("Error reading file (this shouldn't happen): {}", e).red()
                );
            }
            error_stream(stream, 500);
            return;
        }
    }
    stream.flush().unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    return;
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    let re = Regex::new(r"^GET (/.*?) HTTP/(?s).*$").unwrap();

    let listener = TcpListener::bind(format!("{}:{}", cli.bindto, cli.port))?;

    println!("Serving on: {}", listener.local_addr()?);

    let mut requests: HashMap<IpAddr, u64> = HashMap::new();
    let mut lastminute = Utc::now().minute();
    let mut ratelimits: HashMap<IpAddr, DateTime<Utc>> = HashMap::new();

    for stream in listener.incoming() {
        // Rate limiting
        if cli.ratelimit > 0 {
            let ip = stream.as_ref().unwrap().peer_addr()?.ip();
            let now = Utc::now();
            if ratelimits.contains_key(&ip) {
                if now > ratelimits[&ip] {
                    ratelimits.remove(&ip);
                } else {
                    let left = (ratelimits[&ip] - now).num_seconds();
                    stream.as_ref().unwrap().write(format!("HTTP/1.1 429 Too Many Requests\nRetry-After: {}\n\n429\n", left).as_bytes())?;
                    stream.as_ref().unwrap().flush()?;
                    stream.as_ref().unwrap().shutdown(Shutdown::Both)?;
                    if cli.verbose {
                        println!("{}", format!("Rejecting request from rate-limited ip: {}. {} secs left on ratelimit.", ip, left).blue());
                    }
                    continue;
                }
            }
            if now.minute() != lastminute {
                lastminute = now.minute();
                requests.clear();
                println!("{}", "Request count reset.".blue());
            } else {
                if requests.contains_key(&ip) {
                    requests.insert(ip, requests[&ip]+1);
                } else {
                    requests.insert(ip, 1);
                }
                if requests[&ip] >= cli.ratelimit.into() {
                    if !cli.zerologs {
                        println!("{}", format!("Rate limiting {} after {} requests in a minute.", &ip.to_string(), requests[&ip]).red());
                    }
                    ratelimits.insert(ip, now.checked_add_signed(Duration::seconds(cli.timeout)).unwrap());
                    requests.remove(&ip);
                    continue;
                }
            }
        }

        // Handler
        handle_client(stream?, &re, cli.zerologs);
    }
    Ok(())
}
