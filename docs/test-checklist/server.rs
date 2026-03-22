//! Minimal checklist server. Serves index.html and saves checkbox state.
//! Compile: rustc server.rs -o server
//! Run: ./server

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

const PORT: u16 = 8111;

fn main() {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let listener = TcpListener::bind(("127.0.0.1", PORT)).unwrap_or_else(|e| {
        eprintln!("Failed to bind to port {PORT}: {e}");
        std::process::exit(1);
    });

    println!("Checklist running at http://localhost:{PORT}");

    for stream in listener.incoming().flatten() {
        if let Err(e) = handle_connection(stream, &dir) {
            eprintln!("Request error: {e}");
        }
    }
}

fn handle_connection(mut stream: std::net::TcpStream, dir: &Path) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    match (method, path) {
        ("POST", "/save") => {
            // Extract body after \r\n\r\n
            let body = request
                .find("\r\n\r\n")
                .map(|i| &request[i + 4..])
                .unwrap_or("");
            fs::write(dir.join("state.json"), body.as_bytes())?;
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
            stream.write_all(response.as_bytes())?;
        }
        ("GET", "/") | ("GET", "/index.html") => {
            serve_file(&mut stream, &dir.join("index.html"), "text/html")?;
        }
        ("GET", "/state.json") => {
            let state_path = dir.join("state.json");
            if state_path.exists() {
                serve_file(&mut stream, &state_path, "application/json")?;
            } else {
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
                stream.write_all(response.as_bytes())?;
            }
        }
        _ => {
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot found";
            stream.write_all(response.as_bytes())?;
        }
    }
    Ok(())
}

fn serve_file(stream: &mut std::net::TcpStream, path: &Path, content_type: &str) -> std::io::Result<()> {
    match fs::read(path) {
        Ok(body) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes())?;
            stream.write_all(&body)?;
        }
        Err(_) => {
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot found";
            stream.write_all(response.as_bytes())?;
        }
    }
    Ok(())
}
