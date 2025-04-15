use colored::Colorize;
use regex::Regex;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{PathBuf, absolute};

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    // Bind IP Address
    #[arg(default_value_t = String::from("127.0.0.1"))]
    bindto: String,
    // Bind Port
    #[arg(default_value_t = 8080)]
    port: u16,
}

fn error_stream(mut stream: TcpStream, error_id: u16) {
    stream
        .write(format!("HTTP/1.1 {} Bad Request\n\n{}", error_id, error_id).as_bytes())
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

fn handle_client(mut stream: TcpStream, header_regex: &Regex) {
    let peer = stream.peer_addr().unwrap();
    //println!("Connection from {}", peer.to_string());
    
    let mut buffer: [u8; 4096] = [0; 4096];
    stream.read(&mut buffer).unwrap();

    let header = String::from_utf8_lossy(&buffer);

    if !header_regex.is_match(&header) {
        println!("{}", "GET - 400".yellow());
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
        print_message(peer.ip().to_string(), &m[1], 404);
        error_stream(stream, 404);
        return;
    }

    match path.canonicalize() {
        Ok(_path) => {
            path = _path;
        }
        Err(_) => {
            println!(
                "{}",
                format!("!!! TOCTOU Prevented: {} !!!", path.display()).red()
            );
            error_stream(stream, 404);
            return;
        }
    }

    // Protection from directory escape
    if !path.starts_with(PathBuf::from(".").canonicalize().unwrap()) {
        println!(
            "{}",
            format!("!!! Directory escape prevented: {} !!!", path.display()).red()
        );
        error_stream(stream, 404);
        return;
    }

    let file = fs::read(path);

    match file {
        Ok(file) => {
            print_message(peer.ip().to_string(), &m[1], 200);
            stream.write_all(b"HTTP/1.1 200 OK\n\n").unwrap();
            stream.write_all(&*file).unwrap();
        }
        Err(e) => {
            println!(
                "{}",
                format!("Error reading file (this shouldn't happen): {}", e).red()
            );
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

    for stream in listener.incoming() {
        handle_client(stream?, &re);
    }
    Ok(())
}
