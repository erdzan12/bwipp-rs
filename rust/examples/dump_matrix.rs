//! Dump a barcode's pixel/bar matrix as JSON. Used for oracle
//! comparison against bwip-js. Internal tool — not shipped.
use bwipp::{Encoded, Options, Symbology};

fn main() {
    let mut args = std::env::args().skip(1);
    let sym_id = args.next().expect("usage: dump_matrix <symbology> <data>");
    let data = args.next().expect("usage: dump_matrix <symbology> <data>");
    let sym = Symbology::from_id(&sym_id).expect("unknown symbology");
    let r = sym.encode(&data, &Options::default()).expect("encode");
    match r {
        Encoded::Matrix(bm) => {
            print!("{{\"size\":{},\"matrix\":[", bm.width());
            for y in 0..bm.height() {
                if y > 0 {
                    print!(",");
                }
                print!("[");
                for x in 0..bm.width() {
                    if x > 0 {
                        print!(",");
                    }
                    print!("{}", if bm.get(x, y) { 1 } else { 0 });
                }
                print!("]");
            }
            println!("]}}");
        }
        _ => panic!("expected matrix output"),
    }
}
