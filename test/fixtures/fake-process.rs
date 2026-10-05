//! Offline process fixture. Compile with the same Rust toolchain as the tests.
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::time::Duration;

fn option(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn quoted(value: &str) -> String {
    let mut result = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            value if value.is_control() => {
                result.push_str(&format!("\\u{:04x}", u32::from(value)));
            }
            value => result.push(value),
        }
    }
    result.push('"');
    result
}

fn checkpoint(address: Option<&str>, child_id: Option<u32>) {
    if let Some(address) = address {
        let mut control = TcpStream::connect(address).expect("fake control connection");
        control
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        writeln!(control, "READY {} {}", process::id(), child_id.unwrap_or(0)).unwrap();
        let mut signal = [0];
        control
            .read_exact(&mut signal)
            .expect("fake release signal");
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let scenario = args.first().map(String::as_str).unwrap_or("success");
    let address = option(&args, "--control");
    let output = option(&args, "--output").map(PathBuf::from);
    match scenario {
        "child-hang" => {
            println!("CHILD_READY");
            io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "spawn-child" => {
            let parent_exits = args.iter().any(|argument| argument == "--parent-exits");
            let mut child = Command::new(env::current_exe().unwrap())
                .arg("child-hang")
                .stdout(if parent_exits {
                    Stdio::inherit()
                } else {
                    Stdio::piped()
                })
                .stderr(if parent_exits {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .spawn()
                .unwrap();
            if !parent_exits {
                let mut line = String::new();
                BufReader::new(child.stdout.take().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                assert_eq!(line.trim(), "CHILD_READY");
            }
            checkpoint(address.as_deref(), Some(child.id()));
            if parent_exits {
                return;
            }
            child.kill().unwrap();
            child.wait().unwrap();
        }
        "hang" => {
            checkpoint(address.as_deref(), None);
            if address.is_none() {
                loop {
                    std::thread::park();
                }
            }
        }
        "slow-output" => {
            println!("[download] 10.0% at 1.0MiB/s ETA 00:10");
            io::stdout().flush().unwrap();
            checkpoint(address.as_deref(), None);
            println!("[download] 100.0% at 1.0MiB/s ETA 00:00");
        }
        "huge-output" => {
            checkpoint(address.as_deref(), None);
            let stdout = vec![b'o'; 8192];
            let stderr = vec![b'e'; 8192];
            for _ in 0..128 {
                io::stdout().write_all(&stdout).unwrap();
                io::stderr().write_all(&stderr).unwrap();
            }
        }
        "malformed-output" => {
            checkpoint(address.as_deref(), None);
            io::stdout()
                .write_all(b"pinefetch_metadata:{not-json}\n\xff\xfe\n")
                .unwrap();
        }
        "partial-output" => {
            checkpoint(address.as_deref(), None);
            io::stdout()
                .write_all(b"[download] 42.0% at 2.1MiB/s ETA 00:13")
                .unwrap();
            io::stderr().write_all(b"ERROR: interrupted").unwrap();
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
            process::exit(7);
        }
        "exit-error" => {
            checkpoint(address.as_deref(), None);
            eprintln!("ERROR: controlled external process failure");
            process::exit(23);
        }
        "success" => {
            checkpoint(address.as_deref(), None);
            println!("[download] 42.0% at 2.1MiB/s ETA 00:13");
            if let Some(path) = output {
                fs::write(&path, b"PineFetch synthetic output\n").unwrap();
                let path = path.to_string_lossy();
                println!("{path}");
                println!(
                    "pinefetch_metadata:{{\"filepath\":{},\"title\":\"Offline clip\",\"uploader\":\"Fixture creator\",\"duration\":13}}",
                    quoted(&path)
                );
                println!(
                    "pinefetch_caption:{{\"filepath\":{},\"description\":\"Offline caption\"}}",
                    quoted(&path)
                );
            }
            eprintln!("fixture diagnostic");
        }
        other => {
            eprintln!("Unknown fake scenario: {other}");
            process::exit(64);
        }
    }
}
