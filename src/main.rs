#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::{fs, fs::File};
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::path::{PathBuf, absolute};
use std::process::exit;
use std::thread;
use chrono::{DateTime, Duration, Timelike, Utc};
use clap::Parser;

use simplelog::*;


const EXITONEXIT: bool = true;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    // Bind IP Address
    #[arg(default_value = "127.0.0.1")]
    bindto: String,
    // Bind Port
    #[arg(default_value_t = 8080)]
    port: u16,
    #[arg(
        short = 'q',
        long,
        default_value_t = false,
        help = "Disable logging.",
        conflicts_with = "verbose"
    )]
    quiet: bool,
    #[arg(
        short = 'v',
        long,
        default_value_t = false,
        help = "Use verbose output",
        conflicts_with = "quiet"
    )]
    verbose: bool,
    #[arg(long, default_value_t = false, help = "Use log files in addition to logging on stdout/err")]
    enablelogfiles: bool,
    #[arg(
        short = 'r',
        long,
        default_value_t = 120,
        help = "Maximum requests per minute before rate-limiting. 0 to disable"
    )]
    ratelimit: u16,
    #[arg(
        short = 'd',
        long,
        default_value_t = 180,
        help = "Timeout in seconds after exceeding ratelimit"
    )]
    timeout: i64,
    #[arg(short = 'b', long, help="Files to blacklist from serving. (Defaults to log files)")]
    blacklist: Option<Vec<String>>,
    #[arg(long, default_value_t = false, help="Indicates that the program is being in test mode.")]
    testing: bool
}

fn error_stream(mut stream: TcpStream, error_id: u16) {
    // These calls don't "need" to succeed. It would just be nice if they did. That's why we use unwrap_or_default
    stream
        .write_all(format!("HTTP/1.1 {error_id} Bad Request\n\n{error_id}\n").as_bytes())
        .unwrap_or_default();
    stream.flush().unwrap_or_default();
    stream.shutdown(Shutdown::Both).unwrap_or_default();
}

fn print_message(ip: &str, path: &str, error_id: u16) {
    if error_id == 200 {
        trace!("{ip}: GET {path} - {error_id}");
    } else {
        info!("{ip}: GET {path} - {error_id}");
    }
}

fn handle_client(mut stream: TcpStream, blacklist: &[PathBuf]) {
    static HEADER_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^GET (/.*?) HTTP/(?s).*$").unwrap());

    let peer = stream.peer_addr().unwrap();
    //println!("Connection from {}", peer.to_string());

    let mut buffer: [u8; 4096] = [0; 4096];
    let _ = stream.read(&mut buffer).unwrap_or_default();

    let header = String::from_utf8_lossy(&buffer);

    if !HEADER_REGEX.is_match(&header) {
        warn!("Malformed request from {}:\n{header}", peer.ip());
        error_stream(stream, 400);
        return;
    }

    let m = HEADER_REGEX.captures(&header).unwrap();

    if EXITONEXIT && &m[1] == "/exit" {
        exit(0);
    }

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
        print_message(&peer.ip().to_string(), &m[1], 404);
        error_stream(stream, 404);
        return;
    }

    if let Ok(path_) = path.canonicalize() {
        path = path_;
    } else {
        error!("!!! TOCTOU Prevented: {} !!!", path.display());
        error_stream(stream, 404);
        return;
    }

    // Protection from directory escape
    if !path.starts_with(PathBuf::from(".").canonicalize().unwrap()) {
        error!("!!! Directory escape prevented: {} !!!", path.display());
        error_stream(stream, 404);
        return;
    }

    // Blacklisting
    if blacklist.contains(&path){
        warn!("Blacklisted file requested: {}", path.display());
        error_stream(stream, 404);
        return;
    }

    let file = fs::read(path);

    match file {
        Ok(file) => {
            print_message(&peer.ip().to_string(), &m[1], 200);
            stream.write_all(b"HTTP/1.1 200 OK\n\n").unwrap_or_default();
            stream.write_all(&file).unwrap_or_default();
        }
        Err(e) => {
            error!("Error reading file (this shouldn't happen): {e}");
            error_stream(stream, 500);
            return;
        }
    }
    stream.flush().unwrap_or_default();
    stream.shutdown(Shutdown::Both).unwrap_or_default();
}

fn main() -> std::io::Result<()> {

    let cli = Cli::parse();

    // We need to do this ASAP
    if cli.testing {
        let oldhook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            oldhook(info);
            exit(1);
        }));
    }

    let logconfig = ConfigBuilder::new()
        .set_time_format_custom(format_description!(version = 2, "[weekday repr:short] [month repr:short] [day] [hour repr:12]:[minute]:[second] [period case:upper] [year repr:full]"))
        .build();

    if cli.enablelogfiles {
        CombinedLogger::init(
            vec![
                TermLogger::new(if cli.quiet {LevelFilter::Off} else {LevelFilter::Info},   logconfig.clone(), TerminalMode::Mixed, ColorChoice::Auto),
                WriteLogger::new(LevelFilter::Debug, logconfig.clone(), File::create("SimpleWebServer.log")?),
                WriteLogger::new(LevelFilter::Trace, logconfig, File::create("SimpleWebServer-FULL.log")?),
            ]
        ).unwrap();
    } else if !cli.quiet {
        TermLogger::init(if cli.verbose {LevelFilter::Trace} else {LevelFilter::Info}, logconfig, TerminalMode::Mixed, ColorChoice::Auto).unwrap();
    }

    //let re = Regex::new(r"^GET (/.*?) HTTP/(?s).*$").unwrap();

    let listener = TcpListener::bind(format!("{}:{}", cli.bindto, cli.port))?;

    info!("Serving on: {}", listener.local_addr()?);

    let mut requests: HashMap<IpAddr, u64> = HashMap::new();
    let mut lastminute = Utc::now().minute();
    let mut ratelimits: HashMap<IpAddr, DateTime<Utc>> = HashMap::new();

    info!("Parsing blacklist...");
    let mut blist = cli.blacklist.unwrap_or_else(|| vec!["SimpleWebServer.log".parse().unwrap(), "SimpleWebServer-FULL.log".parse().unwrap()]);
    let mut normalizedblist = Vec::new();

    // Allow for empty blacklist with -b ""
    if blist.contains(&String::new()) && blist.len() == 1{
        blist.pop();
    }

    {
        let thispath = PathBuf::from(".").canonicalize()?;
        for b in &blist {
            let mut np = thispath.clone();
            np.push(b);
            normalizedblist.push(np);
        }
    }

    info!("Blacklist: {:?}", normalizedblist);
    if blist.is_empty() {
        warn!("Blacklist is empty, log files are exposed.");
    }

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
                    stream.as_ref().unwrap().write_all(
                        format!("HTTP/1.1 429 Too Many Requests\nRetry-After: {left}\n\n429\n",)
                            .as_bytes(),
                    )?;
                    stream.as_ref().unwrap().flush()?;
                    stream.as_ref().unwrap().shutdown(Shutdown::Both)?;
                    debug!("Rejecting request from rate-limited ip: {ip}. {left} secs left on ratelimit.");
                    continue;
                }
            }
            if now.minute() == lastminute {
                if requests.contains_key(&ip) {
                    requests.insert(ip, requests[&ip] + 1);
                } else {
                    requests.insert(ip, 1);
                }
                if requests[&ip] >= cli.ratelimit.into() {
                    warn!("Rate limiting {} after {} requests in a minute.", &ip.to_string(), requests[&ip]);
                    ratelimits.insert(
                        ip,
                        now.checked_add_signed(Duration::seconds(cli.timeout))
                            .unwrap(),
                    );
                    requests.remove(&ip);

                    let left = (ratelimits[&ip] - now).num_seconds();
                    stream.as_ref().unwrap().write_all(
                        format!("HTTP/1.1 429 Too Many Requests\nRetry-After: {left}\n\n429\n")
                            .as_bytes(),
                    )?;
                    stream.as_ref().unwrap().flush()?;
                    stream.as_ref().unwrap().shutdown(Shutdown::Both)?;
                    debug!("Rejecting request from rate-limited ip: {ip}. {left} secs left on ratelimit.");
                    continue;
                }
            } else {
                lastminute = now.minute();
                requests.clear();
                trace!("Request count reset.");
            }
        }
        let b2 = normalizedblist.clone();
        // Handler
        thread::spawn(move || handle_client(stream.unwrap(), &b2));
        //handle_client(stream?, cli.zerologs);
    }
    Ok(())
}
