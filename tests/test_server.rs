// tests/test_server.rs
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::panic;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

struct Server {
    child: Child,
    port: u16,
}

fn getserver(args: &[&str]) -> Server {
    static SERVER_BINARY: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
        let mut path = std::env::current_exe().unwrap();
        assert!(path.pop());
        if path.ends_with("deps") {
            assert!(path.pop());
        }

        // Note: Cargo automatically builds this binary for integration tests.
        path.push(format!(
            "{}{}",
            env!("CARGO_PKG_NAME"),
            std::env::consts::EXE_SUFFIX
        ));
        path
    });

    let port = port_check::free_local_ipv4_port().unwrap();

    println!("Server port: {port}");

    let child = Command::new(SERVER_BINARY.as_path())
        .env_clear()
        .args(["127.0.0.1", port.to_string().as_str()])
        .args(args)
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(100));

    Server { child, port }
}

/// This is fine to call multiple times
/// Call this in any functions using threads
fn set_panic_hook() {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        hook(info);
        std::process::exit(1);
    }))
}

fn get_path(path: &str, port: u16) -> TcpStream {
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    conn.write_all(format!("GET {path} HTTP/1.0\n\n").as_bytes())
        .unwrap();
    conn.flush().unwrap();
    conn
}

#[test]
/// Test that concurrency features are working
pub fn test_concurrent() {
    let mut server = getserver(&[]);

    set_panic_hook();

    let handle = thread::spawn(move || {
        // First connection to server. If server is not running in concurrent mode, it will block until this connection closes
        let connection1 = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        // Second connection to server. Sends get request to path /
        let mut connection2 = get_path("/", server.port);

        /*
         * If server is running in concurrent mode, it will fulfill this request right now.
         * If not, it would be waiting for the first request to finish and would deadlock.
         * To avoid this deadlock, we run this in a separate thread that will be killed when we panic.
         */
        let result = connection2.read(&mut Vec::new());

        connection1.shutdown(Shutdown::Both).unwrap();
        connection2.shutdown(Shutdown::Both).unwrap();

        assert!(result.is_ok());
        println!("Server read result: {result:?}");
    });

    thread::sleep(Duration::from_millis(10));

    server.child.kill().unwrap();

    if !handle.is_finished() {
        panic!("Concurrency is not working!");
    }
    println!("Concurrency is working!");
}

#[test]
pub fn test_404() {
    let mut server = getserver(&[]);
    let mut buf: [u8; 27] = [0; 27];

    let _response = get_path("/invalid", server.port).read(&mut buf);

    server.child.kill().unwrap();

    assert_eq!(
        String::from_utf8_lossy(&buf),
        "HTTP/1.1 404 Not Found\n\n404"
    );
}

#[test]
pub fn test_ratelimiting_1() {
    let mut server = getserver(&["-r", "3", "-d", "2", "-v"]);

    for _ in 1..=2 {
        let mut conn = get_path("/", server.port);
        let mut buf: [u8; 9] = [0; 9];
        let _ = conn.read(&mut buf).unwrap();
        assert_eq!(Vec::from(buf), b"HTTP/1.1 ");
    }

    let mut ratelimited = get_path("/", server.port);

    let mut buf: [u8; 50] = [0; 50];
    let _ = ratelimited.read(&mut buf).unwrap();

    server.child.kill().unwrap();

    assert_eq!(
        Vec::from(buf),
        b"HTTP/1.1 429 Too Many Requests\nRetry-After: 2\n\n429"
    );
}

// TEST OLD EXPLOITS

#[test]
/// Ported from `exploit-0.0.1.sh`
pub fn test_toctou_patched() {
    const TOCTOU_TEST_LENGTH: u8 = 5;

    if Path::new("index.html").exists() {
        println!("index.html already exists!");
        println!("Consider this a skip.");
        return;
    }

    // We disable rate-limiting on the server
    let mut server = getserver(&["--testing", "-r", "0"]);

    let start = time::OffsetDateTime::now_utc();

    loop {
        // Equivalent to `touch index.html`
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open("index.html")
            .unwrap();

        // Here, if following along in the shell script, we are interweaving the curl with the rm
        let mut conn = get_path("/", server.port);

        // This would be the `sleep 0.0015`
        thread::sleep(Duration::from_micros(1000));

        std::fs::remove_file("index.html").unwrap();

        // Wait for server to finish processing. (This would be the `wait $pid` line)
        let _ = conn.read(&mut Vec::new()).unwrap();

        // Break out of the loop if time has expired
        if time::OffsetDateTime::now_utc() - start
            > time::Duration::seconds(TOCTOU_TEST_LENGTH as i64)
        {
            break;
        }

        assert!(
            server.child.try_wait().unwrap().is_none(),
            "TOCTOU is not patched server-side!"
        );
    }

    server.child.kill().unwrap();
}

#[test]
/// Ported from `exploit-0.1.0.py`
pub fn test_incorrect_connection_handling() {
    let mut server = getserver(&["--testing"]);

    let mut conn = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    conn.flush().unwrap();
    conn.shutdown(Shutdown::Both).unwrap();

    assert!(
        server.child.try_wait().unwrap().is_none(),
        "Connection handling bug is not patched server-side!"
    );

    server.child.kill().unwrap();
}

#[test]
/// Ported from `exploit-2.2.0.sh`
pub fn test_exitflag_off() {
    let mut server = getserver(&[]);

    let _conn = get_path("/exit", server.port);

    assert!(
        server.child.try_wait().unwrap().is_none(),
        "EXITFLAG is enabled."
    );
}
