// Only use on nightly
#![cfg_attr(on_nightly, feature(normalize_lexically))]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]
// Restrictions
#![deny(clippy::allow_attributes)]
#![deny(clippy::allow_attributes_without_reason)]
#![deny(clippy::cfg_not_test)]
#![deny(clippy::unwrap_used)]

use clap::Parser;
use regex::Regex;
use simplelog::*;
use std::collections::HashMap;
use std::io::BufReader;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf, absolute};
use std::process::exit;
use std::{fs, fs::File, io, thread};
use time::{Duration, OffsetDateTime};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Needed for the CLI. Cannot be refactored into a state machine."
)]
struct Cli {
    /// Bind IP Address
    #[arg(default_value = "127.0.0.1")]
    address: String,
    /// Bind Port
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
    // Only available on nightly
    #[cfg(on_nightly)]
    #[arg(
        long,
        default_value_t = false,
        help = "Allow serving symlinks that point out of the base directory"
    )]
    allow_external_symlinks: bool,
}

fn error_stream(stream: &mut TcpStream, error_id: u16) {
    if match error_id {
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
    .is_err()
    {
        error!("Could not write error code to stream.");
    }
    if stream.flush().is_err() {
        error!("Failed flushing stream.");
    }
    if stream.shutdown(Shutdown::Both).is_err() {
        error!("Failed closing stream.");
    }
}

fn print_message(ip: &str, path: &str, error_id: u16) {
    if error_id == 200 {
        trace!("{ip}: GET {path} - {error_id}");
    } else {
        info!("{ip}: GET {path} - {error_id}");
    }
}

fn get_path(stream: &mut TcpStream, peer: &IpAddr) -> Option<String> {
    static HEADER_REGEX: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"^GET (/.*?)(?:\?.*)? HTTP/(?s).*$").expect("Unable to create regex")
    });

    //println!("Connection from {}", peer.to_string());

    let mut buffer: [u8; 4096] = [0; 4096];
    if stream.read(&mut buffer).is_err() {
        error!("Could not read get request.");
    }

    let header = String::from_utf8_lossy(&buffer);

    if !HEADER_REGEX.is_match(&header) {
        warn!("Malformed request from {peer}:\n{header}");
        error_stream(stream, 400);
        return None;
    }

    let m = HEADER_REGEX
        .captures(&header)
        .expect("Could not get captures from regex");

    Some(m[1].to_string())
}

fn server_path_to_local_path(requested_path: &str) -> Option<(PathBuf, PathBuf)> {
    // Path parsing
    let Ok(mut path) = absolute(PathBuf::from(&requested_path)) else {
        error!("Could not get absolute path of {requested_path}.");
        return None;
    };

    let path_root = if cfg!(windows) { "C:\\" } else { "/" };

    #[expect(clippy::cmp_owned, reason = "Need to make it a PathBuf to compare.")]
    if path == PathBuf::from(path_root) {
        // If requesting root, change to index.html
        path.push("index.html");
    }

    // Convert into a relative path
    path = PathBuf::from(if let Ok(stripped) = path.strip_prefix(path_root) {
        stripped
    } else {
        error!(
            "Could not strip cwd (convert into relative path): {}",
            path.display()
        );
        return None;
    });
    // Trying adding .html after original request 404s
    if !path.exists() && path.extension().is_none() {
        trace!(
            "{} not found. Using {}.html instead",
            path.display(),
            path.display()
        );
        // Add .html to non html paths
        path.set_extension("html");
    }

    let Ok(abpath) = absolute(&path) else {
        error!("Could not get absolute path of file: {}", path.display());
        return None;
    };

    path.canonicalize()
        .map_or(None, |canon| Some((canon, abpath)))
}

#[cfg(not(on_nightly))]
fn check_path(path: &Path, _: &Path, _: bool) -> bool {
    path.starts_with(if let Ok(cwd_canon) = PathBuf::from(".").canonicalize() {
        cwd_canon
    } else {
        error!("Could not find the current directory. Is someone tampering???");
        return false;
    })
}

#[cfg(on_nightly)]
fn check_path(path: &Path, abpath: &Path, allow_symlinks: bool) -> bool {
    if allow_symlinks && abpath.is_symlink() {
        // This is why we need nightly: for normalize_lexically
        let Ok(ab_sym) = abpath.normalize_lexically() else {
            error!("Could not normalize path!");
            return false;
        };
        // Now just make sure the symlink itself is within our dir
        if ab_sym.starts_with(if let Ok(cwd_canon) = PathBuf::from(".").canonicalize() {
            cwd_canon
        } else {
            error!("Could not find the current directory. Is someone tampering???");
            return false;
        }) {
            info!(
                "Redirecting symlink {} to {}.",
                ab_sym.display(),
                path.display()
            );
            true
        } else {
            false
        }
    } else {
        path.starts_with(if let Ok(cwd_canon) = PathBuf::from(".").canonicalize() {
            cwd_canon
        } else {
            error!("Could not find the current directory. Is someone tampering???");
            return false;
        })
    }
}

fn serve_local_file(
    path: &PathBuf,
    stream: &mut TcpStream,
    peer: &IpAddr,
    blacklist: &[PathBuf],
    requested_path: &str,
    abpath: &Path,
    allow_symlinks: bool,
) -> Result<(), ()> {
    // Protection from directory escape
    if !check_path(path, abpath, allow_symlinks) {
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

    let file = File::open(path);

    if let Ok(file) = file {
        let mut buffer_file = BufReader::new(file);
        print_message(&peer.to_string(), requested_path, 200);
        if stream.write_all(b"HTTP/1.1 200 OK\n\n").is_err() {
            error!("Could not write header to stream.");
        }
        if io::copy(&mut buffer_file, stream).is_err() {
            error!("Error serving file: {}", path.display());
        }
        //stream.write_all(&file).unwrap_or_default();
        Ok(())
    } else {
        // This state will most likely occur if someone is maliciously manipulating files on the host.
        error_stream(stream, 404);
        error!("!!! TOCTOU Prevented: {} !!!", path.display());
        Err(())
    }
}

fn serve_dir_listing(
    stream: &mut TcpStream,
    blacklist: &[PathBuf],
    requested_path: &str,
    actual_path: Option<&str>,
) -> Result<(), ()> {
    // Don't look at this too much. It will hurt you
    if let Ok(files) = fs::read_dir(actual_path.unwrap_or(".")).map(|d| {
        d.map(|f| {
            f.map(|e| {
                //trace!("Path is: {:?}", &e.path().canonicalize());
                // Check against canonicalized path if possible. Otherwise just relative path
                if blacklist.contains(&e.path().canonicalize().unwrap_or_else(|_| e.path())) {
                    "\\//\\".parse().unwrap()
                } else {
                    e.file_name()
                }
            })
        })
    }) {
        let files = files.collect::<Result<Vec<_>, _>>().unwrap_or_default();

        let lis = files
            .iter()
            .map(|f| {
                //trace!("F is {:?}", f);
                if f == "\\//\\" {
                    "".parse().unwrap()
                } else {
                    format!(
                        "<li><a href=\"{}{}{}\">{}</a></li>",
                        if requested_path == "/" {
                            ""
                        } else {
                            requested_path
                        },
                        "/",
                        f.display(),
                        f.display()
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let dir_list = format!(
            include_str!("dirlist.html"),
            directory = requested_path,
            lis = lis
        );

        debug!("Serving dir listing of {}", actual_path.unwrap_or("."));
        if stream.write_all(b"HTTP/1.1 200 OK\n\n").is_err() {
            error!("Could not write header to stream.");
        }
        if stream.write_all(dir_list.as_ref()).is_err() {
            error!("Could not write dirlist to stream.");
        }
    } else {
        error_stream(stream, 500);
        return Err(());
    }

    Ok(())
}

fn handle_client(stream: &mut TcpStream, blacklist: &[PathBuf], allow_symlinks: bool) {
    let peer = stream.peer_addr().map_or_else(
        |_| {
            error!("Could not get peer ip");
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        },
        |addr| addr.ip(),
    );

    let requested_path;

    if let Some(path_) = get_path(stream, &peer) {
        requested_path = path_;
    } else {
        return;
    }

    // Testing if the path exists
    if let Some((path, abpath)) = server_path_to_local_path(&requested_path) {
        serve_local_file(
            &path,
            stream,
            &peer,
            blacklist,
            &requested_path,
            &abpath,
            allow_symlinks,
        )
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

    let clilevel = if cli.quiet {
        LevelFilter::Off
    } else if cli.verbose {
        LevelFilter::Trace
    } else {
        LevelFilter::Info
    };

    if cli.enablelogfiles {
        CombinedLogger::init(vec![
            TermLogger::new(
                clilevel,
                logconfig.clone(),
                TerminalMode::Mixed,
                ColorChoice::Auto,
            ),
            WriteLogger::new(
                LevelFilter::Debug,
                logconfig.clone(),
                File::create("SimpleWebServer.log").expect("Could not create log file"),
            ),
            WriteLogger::new(
                LevelFilter::Trace,
                logconfig,
                File::create("SimpleWebServer-FULL.log").expect("Could not create log file"),
            ),
        ])
        .expect("Could not start logger");
    } else if !cli.quiet {
        TermLogger::init(clilevel, logconfig, TerminalMode::Mixed, ColorChoice::Auto)
            .expect("Could not start logger");
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
        let thispath = PathBuf::from(".")
            .canonicalize()
            .expect("Could not find current directory.");
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
    let Ok(peer_addr) = stream.peer_addr() else {
        error!("Could not get peer IP address.");
        return false;
    };
    let ip = peer_addr.ip();
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
                    .unwrap_or_else(|| {
                        error!("Could not calculate when ratelimit should expire???");
                        // Just let the request through I guess?
                        now
                    }),
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

    let listener = TcpListener::bind(format!("{}:{}", cli.address, cli.port))?;

    info!("Serving on: {}", listener.local_addr()?);

    let mut requests: HashMap<IpAddr, u64> = HashMap::new();
    let mut lastminute = OffsetDateTime::now_local()
        .expect("Could not get the current time")
        .minute();
    let mut ratelimits: HashMap<IpAddr, OffsetDateTime> = HashMap::new();

    let mut normalizedblist: Vec<PathBuf> = Vec::new();

    let ratelimit = cli.ratelimit;
    let timeout = cli.timeout;

    setup_blacklist(cli.blacklist, &mut normalizedblist);
    info!("Blacklist: {:?}", normalizedblist);
    if cli.enablelogfiles && normalizedblist.is_empty() {
        warn!("Blacklist is empty, log files could be exposed.");
    }

    #[cfg(on_nightly)]
    let syms = cli.allow_external_symlinks;
    #[cfg(not(on_nightly))]
    let syms = false;

    for mut stream in listener.incoming() {
        // Rate limiting
        if cli.ratelimit > 0
            && !handle_ratelimiting(
                &mut requests,
                &mut lastminute,
                &mut ratelimits,
                stream
                    .as_mut()
                    .expect("Could not get a mutable reference to the stream"),
                ratelimit,
                timeout,
            )
        {
            continue;
        }
        let b2 = normalizedblist.clone();
        // Handler

        // Multithreaded mode:
        thread::spawn(move || {
            handle_client(&mut stream.expect("Could not get the stream"), &b2, syms);
        });
        // Single threaded mode:
        //handle_client(&mut stream?, &b2);
    }
    Ok(())
}
