use anyhow::{Result, Context, anyhow};
use base64::prelude::*;
use clap::Parser;
use colored::*;
use encoding_rs::Encoding;
use quoted_printable::ParseMode;
use regex::{Captures};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::{fs};
use std::path::{Path, PathBuf};
use regex_macro::regex;

fn decode_charset_crate(charset: &str, encoded_text: &Vec<u8>) -> Result<String> {
    let encoder = match Encoding::for_label(charset.to_ascii_lowercase().as_bytes()) {
        Some(encoder) => encoder,
        None => encoding_rs::WINDOWS_1252,
    };

    let (s, _encoding_used, _malformed) = encoder.decode(&encoded_text);

    let out = s.replace("_", " ");

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

fn parse_header_line(header: &str) -> Result<String> {
    let re = regex!(r"=\?([^?]+)\?([^?]+)\?(.*?)\?=");

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

fn read_headers(path: &Path) -> Result<HashMap<String, String>> {
    let mut map: HashMap<String,String> = HashMap::new();

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut last_header = String::new();
    let mut cur = String::new();
    while let Some(Ok(line)) = lines.next() {
        if line == "" {
            if cur.len() > 0 {
                if let Ok(s) = parse_header_line(&cur) {
                    map.insert(String::from(last_header), s);
                }
            }

            break;
        }

        if line.starts_with(" ") {
            cur.push_str(&line[1..]);
            continue;
        }

        if let Some((header, rest)) = String::from(line).split_once(":") {
            if cur.len() > 0 {
                if let Ok(s) = parse_header_line(&cur) {
                    map.insert(String::from(last_header), s);
                }
            }

            last_header = String::from(header);
            cur = String::from(rest);
        }
    }

    return Ok(map);
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

fn main() {
    let args = Args::parse();

    if args.list_colors {
        list_colors();

        return ()
    }

    let mailbox_color = parse_color(&args.mailbox_color).unwrap_or(Color::Magenta);
    let subject_color = parse_color(&args.subject_color).unwrap_or(Color::BrightCyan);
    let from_color = parse_color(&args.from_color).unwrap_or(Color::Cyan);

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
            let headers = read_headers(&file);

            match headers {
                Ok(map) => {
                    let mailbox = basename.color(mailbox_color);
                    let from = map.get("From").unwrap_or(&"no from".to_string()).color(from_color);
                    let subject = map.get("Subject").unwrap_or(&"no subject".to_string()).color(subject_color);

                    println!("{}: {} / {}", mailbox, from, subject);
                },
                Err(e) => {
                    println!("{}: <No subject> ({})", basename, e);
                }
            };
        }
    }
}
