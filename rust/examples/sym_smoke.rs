//! Smoke test: every Symbology variant should encode its default example
//! without errors. Reports per-symbology pass/fail.
use bwipp::{Options, Symbology};

fn main() {
    let symbologies = Symbology::all();
    let total = symbologies.len();
    let mut ok = 0;
    let mut errs = Vec::new();
    for sym in symbologies {
        let example = sym.default_data();
        match sym.encode(example, &Options::default()) {
            Ok(_) => ok += 1,
            Err(e) => errs.push((sym.id().to_string(), e.to_string())),
        }
    }
    println!("smoke: {}/{} symbologies encode default example", ok, total);
    if !errs.is_empty() {
        println!("failures:");
        for (id, err) in &errs {
            println!("  {id}: {err}");
        }
        std::process::exit(1);
    }
}
