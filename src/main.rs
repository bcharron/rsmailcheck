use anyhow::{Result, Context, anyhow};
use base64::prelude::*;
use clap::Parser;
use colored::*;
use encoding_rs::Encoding;
use encoding_rs::WINDOWS_1252;
use quoted_printable::ParseMode;
use regex::{Regex, Captures};
use std::collections::HashMap;
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

fn parse_header(header: &str) -> Result<String> {
    let re = Regex::new(r"=\?([^?]+)\?([^?]+)\?(.*?)\?=")?;

    let output = re.replace_all(header.trim_start(), |caps: &Captures| {
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

#[derive(Debug)]
struct MailInfo {
    from: Option<String>,
    subject: Option<String>,
}

async fn parse_file(path: &Path) -> Result<MailInfo> {
    let contents = fs::read_to_string(path)?;

    let mut subject_lines: Vec<&str> = Vec::new();
    let mut lines = contents.lines().peekable();
    let mut subject = None;
    let mut from = None;
    
    while let Some(&line) = lines.peek() {
        if line.starts_with("Subject:") {
            subject_lines.push(lines.next().unwrap());
            
            while let Some(&next_line) = lines.peek() {
                if next_line.starts_with(" ") {
                    subject_lines.push(lines.next().unwrap());
                } else {
                    break;
                }
            }
            
            let s = subject_lines.concat();
            subject = match parse_header(s.trim_start_matches("Subject:")) {
                Ok(s) => Some(s),
                Err(e) => Some(e.to_string()),
            };
        } else if line.starts_with("From:") {
            from = match parse_header(lines.next().unwrap().trim_start_matches("From:")) {
                Ok(s) => Some(s),
                Err(e) => Some(e.to_string()),
            };
        } else {
            lines.next();
        }
    }
    
    return Ok(MailInfo {
        from,
        subject,
    });

    // return Err(anyhow!("No Subject line in {:?}", path));
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

    /// Color of subject line
    #[arg(short, long, default_value = "light cyan")]
    subject_color: String,

    /// List available colors
    #[arg(short, long)]
    list_colors: bool,

    /// maildir directories
    inputs: Vec<String>,
}

use colored::Color;

fn color_map() -> HashMap<&'static str, Color> {
    use Color::*;
    [
        ("black", Black),
        ("blue", Blue),
        ("bright_black", BrightBlack),
        ("bright_blue", BrightBlue),
        ("bright_cyan", BrightCyan),
        ("bright_green", BrightGreen),
        ("bright_magenta", BrightMagenta),
        ("bright_red", BrightRed),
        ("bright_white", BrightWhite),
        ("bright_yellow", BrightYellow),
        ("cyan", Cyan),
        ("green", Green),
        ("magenta", Magenta),
        ("red", Red),
        ("white", White),
        ("yellow", Yellow),
    ]
    .into_iter()
    .collect()
}

fn parse_color(name: &str) -> Option<Color> {
    color_map().get(&name.to_lowercase().as_str()).copied()
}

fn list_colors() {
    let map = color_map();
    let keys = map.keys().map(|k| k.to_string());
    let mut colors: Vec<String> = keys.collect();

    colors.sort();

    for name in colors {
        println!("  {}", name);
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.list_colors {
        list_colors();

        return ()
    }

    let mailbox_color = parse_color(&args.mailbox_color).unwrap_or(Color::Magenta);
    let subject_color = parse_color(&args.subject_color).unwrap_or(Color::BrightCyan);
    let from_color = parse_color(&args.from_color).unwrap_or(Color::Cyan);

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
                    Ok(m) => {
                        let mailbox = basename.color(mailbox_color);
                        let from = m.from.unwrap_or("no from".to_string()).color(from_color);
                        let subject = m.subject.unwrap_or("no subject".to_string()).color(subject_color);

                        format!("{}: {} / {}", mailbox, from, subject)
                    },
                    Err(e) => format!("{}: <No subject> ({})", basename, e),
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
