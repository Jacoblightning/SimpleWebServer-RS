// tests/test_server.rs
use libc::atexit;
use once_cell::sync::Lazy;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static PORT: Lazy<u16> = Lazy::new(|| fastrand::u16(2..=65535));

pub fn ensure_server_started() {
    static STARTED: Mutex<Option<Child>> = Mutex::new(None);

    println!("Server port: {}", *PORT);

    let mut started = STARTED.lock().unwrap();
    if started.is_some() {
        return;
    }

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

    let mut cmd = Command::new(path);
    cmd.env_clear();
    // For some reason, the tests don't work with these uncommented
    // cmd.stdout(Stdio::piped());
    // cmd.stderr(Stdio::piped());
    // cmd.stdin(Stdio::piped());
    cmd.args(["127.0.0.1", &PORT.to_string()]);


    let server = cmd.spawn().unwrap();

    /*
    let stdout = server.stdout.take().unwrap();
    let reader = std::io::BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut running = false;
    while let Some(Ok(line)) = lines.next() {
        if line.contains("Serving") {
            running = true;
            break;
        }
    }

    assert!(running);
     */

    // Give the server 1/2 second to start up
    thread::sleep(Duration::from_millis(500));

    extern "C" fn kill() {
        STARTED.lock().unwrap().as_mut().unwrap().kill().unwrap();
    }

    unsafe { atexit(kill) };

    *started = Some(server);
}

fn get_path(path: &str) -> TcpStream {
    let mut conn = TcpStream::connect(("127.0.0.1", *PORT)).unwrap();
    conn.write_all(format!("GET {path} HTTP/1.0\n\n").as_bytes()).unwrap();
    conn.flush().unwrap();
    conn
}

#[test]
/// Test that concurrency features are working
pub fn test_concurrent() {
    ensure_server_started();

    let handle = thread::spawn(|| {
        // First connection to server. If server is not running in concurrent mode, it will block until this connection closes
        let connection1 = TcpStream::connect(("127.0.0.1", *PORT)).unwrap();
        // Second connection to server. Sends get request to path /
        let mut connection2 = get_path("/");

        /*
         * If server is running in concurrent mode, it will fulfill this request right now.
         * If not, it would be waiting for the first request to finish and would deadlock.
         * To avoid this deadlock, we run this in a separate thread that will be killed when we panic.
        */
        let result = connection2.read(&mut Vec::new());

        connection1.shutdown(Shutdown::Both).unwrap();
        connection2.shutdown(Shutdown::Both).unwrap();

        assert!(result.is_ok());
        println!("Server read result: {:?}", result.unwrap());
    });

    thread::sleep(Duration::from_millis(10));

    if !handle.is_finished() {
        panic!("Concurrency is not working!");
    }
    println!("Concurrency is working!");
}

#[test]
pub fn test_404() {
    ensure_server_started();

    let mut conn = get_path("/invalid");

    let mut buf: [u8; 30] = [0; 30];
    let _response = conn.read(&mut buf);

    assert_eq!(String::from_utf8_lossy(&buf), "HTTP/1.1 404 Bad Request\n\n404\n");
}
