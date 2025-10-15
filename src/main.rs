use anyhow::{Result, Context, anyhow};
use base64::prelude::*;
use clap::Parser;
use encoding_rs::Encoding;
use encoding_rs::WINDOWS_1252;
use quoted_printable::ParseMode;
use regex::{Regex, Captures};
use std::{fs};
use std::path::{Path, PathBuf};
use tokio;

#[allow(dead_code)]
fn latin1_to_string(s: &[u8]) -> String {
    s.iter().map(|&c| c as char).collect()
}

fn decode_charset_crate(charset: &str, encoded_text: &Vec<u8>) -> Result<String> {
    let encoder = match Encoding::for_label(charset.to_ascii_lowercase().as_bytes()) {
        Some(encoder) => encoder,
        None => encoding_rs::WINDOWS_1252,
    };

    let (s, _encoding_used, _malformed) = encoder.decode(&encoded_text);

    let out = s.replace("_", " ");

    return Ok(out);
}

#[allow(dead_code)]
fn decode_charset(charset: &str, encoded_text: &Vec<u8>) -> Result<String> {
    let out = match charset.to_ascii_uppercase().as_str() {
        "UTF-8" => String::from_utf8(encoded_text.to_vec())?,
        "ISO-8859-1" => latin1_to_string(encoded_text).to_string(),
        // "WINDOWS-1252" => latin1_to_string(encoded_text).to_string(),
        "WINDOWS-1252" => {
            let (s1, _encoding_used, _malformed) = WINDOWS_1252.decode(encoded_text);
            s1.into_owned()
        },
        _ => return Err(anyhow!("Could not convert unknown charset {}", charset)),
    };

    let r = out.replace("_", " ");

    return Ok(r);
}

#[allow(dead_code)]
fn decode_quoted<'a>(encoded_text: &'a str) -> Result<Vec<u8>> {
    let bytes = encoded_text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());

    let mut x = 0;
    while x < (bytes.len() - 2) {
        let c = match bytes[x] {
            b'=' => {
                let s = str::from_utf8(&bytes[x+1..x+3]);

                if s.is_ok() {
                    match u8::from_str_radix(s.unwrap(), 16) {
                        Ok(i) => { x += 2; i },
                        Err(_) => b'=',
                    }
                } else {
                    b'='
                }
            }
            c @ _ => c,
        };

        out.push(c);

        x += 1;
    }

    while x < bytes.len() {
        out.push(bytes[x]);
        x += 1;
    }

    return Ok(out);
}

fn decode_base64<'a>(data: &'a str) -> Result<Vec<u8>> {
    BASE64_STANDARD.decode(data).context("base64 error")
}

fn parse_encoding<'a>(charset: &str, encoding: &str, data: &'a str) -> Result<String> {
    let decoded = match encoding.to_ascii_uppercase().as_str() {
        "Q" => quoted_printable::decode(data, ParseMode::Robust).context("Decoding failed"),
        "B" => decode_base64(data),
        v @ _ => Err(anyhow!("Unknown encoding type, {}", v)),
    };
    
    match decoded {
        Ok(v) => decode_charset_crate(charset, &v),
        Err(e) => Err(e),
    }
}

fn parse_subject(line: &str) -> Result<String> {
    let subject = line.trim_start_matches("Subject:");

    let re = Regex::new(r"=\?([^?]+)\?([^?]+)\?(.*?)\?=")?;

    let output = re.replace_all(subject.trim_start(), |caps: &Captures| {
        let (Some(charset), Some(encoding), Some(encoded_text)) = (
            caps.get(1).map(|m| m.as_str()),
            caps.get(2).map(|m| m.as_str()),
            caps.get(3).map(|m| m.as_str()),
        ) else {
            // If any part is missing, return the original match unmodified
            return caps.get(0).map_or("", |m| m.as_str()).to_string();
        };

        match parse_encoding(charset, encoding, encoded_text) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Encoding error: {}", e);
                caps.get(0).map_or("", |m| m.as_str()).to_string()
            },
        }
    });

    return Ok(output.into_owned());
}

async fn parse_file(path: &Path) -> Result<String> {
    let contents = fs::read_to_string(path)?;

    let mut subject_lines: Vec<&str> = Vec::new();
    let mut lines = contents.lines();

    while let Some(line) = lines.next() {
        if line.starts_with("Subject:") {
            subject_lines.push(line);

            for more in lines.by_ref() {
                if more.starts_with(" ") {
                    subject_lines.push(more);
                } else {
                    break;
                }
            }

            let s = subject_lines.concat();
            return parse_subject(s.as_str());
        }
    }

    return Err(anyhow!("No Subject line in {:?}", path));
}

fn find_files(path: &PathBuf) -> Vec<PathBuf> {
    let files = fs::read_dir(path).expect("Failed to read_dir on path");
    let mut paths = Vec::new();

    for file in files {
        match file {
            Ok(f) => paths.push(f.path()),
            Err(_) => (),
        }
    }

    return paths
}

/// View emails in a maildir
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    // Color of From header
    #[arg(short, long, default_value = "cyan")]
    from_color: String,

    /// Color of mailbox name
    #[arg(short, long, default_value = "magenta")]
    mailbox_color: String,

    /// Number of times to greet
    #[arg(short, long, default_value = "light cyan")]
    subject_color: String,

    /// maildir directories
    inputs: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut handles = Vec::new();
    let mut paths = Vec::new();

    for path in args.inputs {
        let mut cur_path = PathBuf::from(&path);
        cur_path.push("cur");
        if cur_path.exists() {
            paths.push(cur_path);
        }

        let mut new_path = PathBuf::from(&path);
        new_path.push("new");
        if new_path.exists() {
            paths.push(new_path);
        }
    }

    for path in paths {
        let basename = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|p| p.to_str())
            .unwrap_or_else(|| path.to_str().unwrap());

            let files = find_files(&path);
    
        for file in files {
                let file = file.clone();
                let basename = basename.to_string();
                let handle = tokio::spawn(async move {
                    let content = parse_file(&file).await;

                    return match content {
                        Ok(s) => format!("{}: {}", basename, s),
                        Err(_) => format!("{}: <No subject>", basename),
                   };
                });
    
                handles.push(handle);
        }
    }

    for handle in handles {
        let content = handle.await;

        match content {
                Ok(line) => println!("{}", line),
                Err(err) => eprintln!("failed with {}", err),
        };
    }
}
