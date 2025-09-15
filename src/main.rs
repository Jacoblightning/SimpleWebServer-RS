#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]

use clap::Parser;
use regex::Regex;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::path::{PathBuf, absolute};
use std::process::exit;
use std::thread;
use std::{fs, fs::File};
use time::{Duration, OffsetDateTime};

use simplelog::*;

const EXITONEXIT: bool = true;

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
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
        help = "Disable logging. (Log files are still used if `--enablelogfiles` is passed)",
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
    #[arg(
        long,
        default_value_t = false,
        help = "Use log files in addition to logging on stdout/err"
    )]
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
    timeout: u32,
    #[arg(
        short = 'b',
        long,
        help = "Files to blacklist from serving. (Defaults to log files)"
    )]
    blacklist: Option<Vec<String>>,
    #[arg(
        long,
        default_value_t = false,
        help = "Indicates that the program is being run in test mode. (You don't need this for normal invocation)"
    )]
    testing: bool,
}

fn error_stream(stream: &mut TcpStream, error_id: u16) {
    // These calls don't "need" to succeed. It would just be nice if they did. That's why we use unwrap_or_default
    match error_id {
        404 => {
            stream.write_all(format!("HTTP/1.1 {error_id} Not Found\n\n{error_id}\n").as_bytes())
        }
        400 => {
            stream.write_all(format!("HTTP/1.1 {error_id} Bad Request\n\n{error_id}\n").as_bytes())
        }
        500 => stream.write_all(
            format!("HTTP/1.1 {error_id} Internal Server Error\n\n{error_id}\n").as_bytes(),
        ),
        _ => stream
            .write_all(format!("HTTP/1.1 {error_id} Unknown Error\n\n{error_id}\n").as_bytes()),
    }
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

fn get_path(stream: &mut TcpStream, peer: &IpAddr) -> Option<String> {
    static HEADER_REGEX: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^GET (/.*?)(?:\?.*)? HTTP/(?s).*$").unwrap());

    //println!("Connection from {}", peer.to_string());

    let mut buffer: [u8; 4096] = [0; 4096];
    let _ = stream.read(&mut buffer).unwrap_or_default();

    let header = String::from_utf8_lossy(&buffer);

    if !HEADER_REGEX.is_match(&header) {
        warn!("Malformed request from {peer}:\n{header}");
        error_stream(stream, 400);
        return None;
    }

    let m = HEADER_REGEX.captures(&header).unwrap();

    Some(m[1].to_string())
}

fn server_path_to_local_path(requested_path: &str) -> Option<PathBuf> {
    // Path parsing
    let mut path: PathBuf = absolute(PathBuf::from(&requested_path)).unwrap();

    let path_root = if cfg!(windows) { "C:\\" } else { "/" };

    if path == PathBuf::from(path_root) {
        // If requesting root, change to index.html
        path.push("index.html");
    }

    // Convert into a relative path
    path = PathBuf::from(path.strip_prefix(path_root).unwrap());

    // Trying adding .html after original request 404s
    if !path.exists() && path.extension().is_none() {
        // Add .html to non html paths
        path.set_extension("html");
    }

    path.canonicalize().ok()
}

fn serve_local_file(
    path: &PathBuf,
    stream: &mut TcpStream,
    peer: &IpAddr,
    blacklist: &[PathBuf],
    requested_path: &str,
) -> Result<(), ()> {
    // Protection from directory escape
    if !path.starts_with(PathBuf::from(".").canonicalize().unwrap()) {
        error_stream(stream, 404);
        error!("!!! Directory escape prevented: {} !!!", path.display());
        return Err(());
    }

    // Blacklisting
    if blacklist.contains(path) {
        error_stream(stream, 404);
        warn!("Blacklisted file requested: {}", path.display());
        return Err(());
    }

    if path.is_dir() {
        // Well, we can't exactly read a dir so instead we serve a dir listing
        return serve_dir_listing(stream, blacklist, requested_path, path.to_str());
    }

    let file = fs::read(path);

    match file {
        Ok(file) => {
            print_message(&peer.to_string(), requested_path, 200);
            stream.write_all(b"HTTP/1.1 200 OK\n\n").unwrap_or_default();
            stream.write_all(&file).unwrap_or_default();
            Ok(())
        }
        // This state will most likely occur if someone is maliciously manipulating files on the host.
        Err(_) => {
            error_stream(stream, 404);
            error!("!!! TOCTOU Prevented: {} !!!", path.display());
            Err(())
        }
    }
}

fn serve_dir_listing(
    stream: &mut TcpStream,
    blacklist: &[PathBuf],
    requested_path: &str,
    actual_path: Option<&str>,
) -> Result<(), ()> {
    // Don't look at this too much. It will hurt you
    if let Ok(files) = fs::read_dir(actual_path.unwrap_or(".")).map(|d| {d.map(|f| {
        f.map(|e| {
            if blacklist.contains(&e.path()) {
                // TODO: Fixme
                "\\//\\".parse().unwrap()
            } else {
                e.file_name()
            }
        })
    })}){
        let files = files.collect::<Result<Vec<_>, _>>().unwrap_or_default();

        let lis = files.iter().map(|f|
            {
                if f == "\\//\\" {
                    "".parse().unwrap()
                } else {
                    format!("<li><a href=\"{}{}{}\">{}</a></li>", if requested_path == "/" {""} else {requested_path}, "/", f.display(), f.display())
                }
            }
        ).collect::<Vec<_>>().join("\n");

        let dir_list = format!(include_str!("dirlist.html"), directory=requested_path, lis=lis);

        stream.write_all(b"HTTP/1.1 200 OK\n\n").unwrap_or_default();
        stream.write_all(dir_list.as_ref()).unwrap_or_default();
    } else {
        error_stream(stream, 500);
        return Err(());
    }

    Ok(())
}

fn handle_client(stream: &mut TcpStream, blacklist: &[PathBuf]) {
    let peer = stream.peer_addr().unwrap().ip();

    let requested_path;

    if let Some(path_) = get_path(stream, &peer) {
        requested_path = path_;
    } else {
        return;
    }

    // For testing purposes
    if EXITONEXIT && requested_path == "/exit" {
        exit(0);
    }

    // Testing if the path exists
    if let Some(path) = server_path_to_local_path(&requested_path) {
        serve_local_file(&path, stream, &peer, blacklist, &requested_path)
            .map(|()| {
                stream.flush().unwrap_or_default();
                stream.shutdown(Shutdown::Both).unwrap_or_default();
            })
            .unwrap_or_default();
    } else if requested_path == if cfg!(windows) { "C:\\" } else { "/" } {
        // Dir listing
        serve_dir_listing(stream, blacklist, &requested_path, None).unwrap_or_default();
    } else {
        error_stream(stream, 404);
        print_message(&peer.to_string(), &requested_path, 404);
    }
}

fn setup_logger(cli: &Cli) {
    let logconfig = ConfigBuilder::new()
        .set_time_format_custom(format_description!(version = 2, "[weekday repr:short] [month repr:short] [day] [hour repr:12]:[minute]:[second] [period case:upper] [year repr:full]"))
        .build();

    if cli.enablelogfiles {
        CombinedLogger::init(vec![
            TermLogger::new(
                if cli.quiet {
                    LevelFilter::Off
                } else {
                    LevelFilter::Info
                },
                logconfig.clone(),
                TerminalMode::Mixed,
                ColorChoice::Auto,
            ),
            WriteLogger::new(
                LevelFilter::Debug,
                logconfig.clone(),
                File::create("SimpleWebServer.log").unwrap(),
            ),
            WriteLogger::new(
                LevelFilter::Trace,
                logconfig,
                File::create("SimpleWebServer-FULL.log").unwrap(),
            ),
        ])
        .unwrap();
    } else if !cli.quiet {
        TermLogger::init(
            if cli.verbose {
                LevelFilter::Trace
            } else {
                LevelFilter::Info
            },
            logconfig,
            TerminalMode::Mixed,
            ColorChoice::Auto,
        )
        .unwrap();
    }
}

fn setup_blacklist(blist: Option<Vec<String>>, normalizedblist: &mut Vec<PathBuf>) {
    info!("Parsing blacklist...");
    let mut blist = blist.unwrap_or_else(|| {
        vec![
            "SimpleWebServer.log".parse().unwrap(),
            "SimpleWebServer-FULL.log".parse().unwrap(),
        ]
    });

    // Allow for empty blacklist with -b ""
    if blist.contains(&String::new()) && blist.len() == 1 {
        blist.pop();
    }

    {
        let thispath = PathBuf::from(".").canonicalize().unwrap();
        for b in &blist {
            let mut np = thispath.clone();
            np.push(b);
            normalizedblist.push(np);
        }
    }
}

// Returns true to allow the request and false to block it
fn handle_ratelimiting(
    requests: &mut HashMap<IpAddr, u64>,
    lastminute: &mut u8,
    ratelimits: &mut HashMap<IpAddr, OffsetDateTime>,
    stream: &mut TcpStream,
    ratelimit: u16,
    timeout: u32,
) -> bool {
    let ip = stream.peer_addr().unwrap().ip();
    let now = OffsetDateTime::now_utc();
    if ratelimits.contains_key(&ip) {
        if now.gt(&ratelimits[&ip]) {
            ratelimits.remove(&ip);
        } else {
            let left = (ratelimits[&ip] - now).whole_seconds();
            stream
                .write_all(
                    format!("HTTP/1.1 429 Too Many Requests\nRetry-After: {left}\n\n429\n",)
                        .as_bytes(),
                )
                .unwrap_or_default();
            stream.flush().unwrap_or_default();
            stream.shutdown(Shutdown::Both).unwrap_or_default();
            debug!("Rejecting request from rate-limited ip: {ip}. {left} secs left on ratelimit.");
            return false;
        }
    }
    if now.minute() == *lastminute {
        if requests.contains_key(&ip) {
            requests.insert(ip, requests[&ip] + 1);
        } else {
            requests.insert(ip, 1);
        }
        if requests[&ip] >= ratelimit.into() {
            warn!(
                "Rate limiting {} after {} requests in a minute.",
                &ip.to_string(),
                requests[&ip]
            );
            ratelimits.insert(
                ip,
                now.checked_add(Duration::seconds(i64::from(timeout)))
                    .unwrap(),
            );
            requests.remove(&ip);

            let left = (ratelimits[&ip] - now).whole_seconds();
            stream
                .write_all(
                    format!("HTTP/1.1 429 Too Many Requests\nRetry-After: {left}\n\n429\n")
                        .as_bytes(),
                )
                .unwrap_or_default();
            stream.flush().unwrap_or_default();
            stream.shutdown(Shutdown::Both).unwrap_or_default();
            debug!("Rejecting request from rate-limited ip: {ip}. {left} secs left on ratelimit.");
            return false;
        }
    } else {
        *lastminute = now.minute();
        requests.clear();
        trace!("Request count reset.");
    }
    true
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

    setup_logger(&cli);

    let listener = TcpListener::bind(format!("{}:{}", cli.bindto, cli.port))?;

    info!("Serving on: {}", listener.local_addr()?);

    let mut requests: HashMap<IpAddr, u64> = HashMap::new();
    let mut lastminute = OffsetDateTime::now_local().unwrap().minute();
    let mut ratelimits: HashMap<IpAddr, OffsetDateTime> = HashMap::new();

    let mut normalizedblist: Vec<PathBuf> = Vec::new();

    let ratelimit = cli.ratelimit;
    let timeout = cli.timeout;

    setup_blacklist(cli.blacklist, &mut normalizedblist);
    info!("Blacklist: {:?}", normalizedblist);
    if cli.enablelogfiles && normalizedblist.is_empty() {
        warn!("Blacklist is empty, log files are exposed.");
    }

    for mut stream in listener.incoming() {
        // Rate limiting
        if cli.ratelimit > 0
            && !handle_ratelimiting(
                &mut requests,
                &mut lastminute,
                &mut ratelimits,
                stream.as_mut().unwrap(),
                ratelimit,
                timeout,
            )
        {
            continue;
        }
        let b2 = normalizedblist.clone();
        // Handler

        // Multithreaded mode:
        thread::spawn(move || handle_client(&mut stream.unwrap(), &b2));
        // Single threaded mode:
        //handle_client(&mut stream?, &b2);
    }
    Ok(())
}
