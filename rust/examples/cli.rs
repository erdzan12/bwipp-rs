//! Tiny CLI for trying the encoder out.
//!
//! Usage:
//!   cargo run --example cli -- <symbology> <data> [png|svg] [output_path]
//!
//! Examples:
//!   cargo run --example cli -- code128 "Hello" svg /tmp/hello.svg
//!   cargo run --example cli -- qrcode "https://example.com" png /tmp/qr.png

use std::env;
use std::fs;
use std::process::ExitCode;

use bwipp::{render_png, render_svg, Options, Symbology};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: cli <symbology> <data> [png|svg] [output_path]");
        eprintln!();
        eprintln!("known symbologies:");
        for s in Symbology::all() {
            eprintln!("  {}", s.id());
        }
        return ExitCode::from(2);
    }

    let sym_id = &args[1];
    let data = &args[2];
    let fmt = args.get(3).map(String::as_str).unwrap_or("svg");
    let default_out = format!("{sym_id}.{fmt}");
    let out_path = args.get(4).map(String::as_str).unwrap_or(&default_out);

    let symbology = match Symbology::from_id(sym_id) {
        Some(s) => s,
        None => {
            eprintln!("unknown symbology: {sym_id}");
            return ExitCode::from(2);
        }
    };

    let opts = Options::default();
    let bytes: Vec<u8> = match fmt {
        "svg" => match render_svg(symbology, data, &opts) {
            Ok(svg) => svg.into_bytes(),
            Err(e) => {
                eprintln!("render error: {e}");
                return ExitCode::from(1);
            }
        },
        "png" => match render_png(symbology, data, &opts) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("render error: {e}");
                return ExitCode::from(1);
            }
        },
        other => {
            eprintln!("unknown format: {other} (try `png` or `svg`)");
            return ExitCode::from(2);
        }
    };

    if let Err(e) = fs::write(out_path, &bytes) {
        eprintln!("write error: {e}");
        return ExitCode::from(1);
    }
    println!("wrote {} bytes to {out_path}", bytes.len());
    ExitCode::SUCCESS
}
