//! MEMORY comparison (peak heap) between 3MF readers.
//!
//! Criterion only measures time, so this is a separate harness: a global
//! allocator (`peak_alloc`) tracks the peak of live bytes, and we measure each
//! reader independently over the same file. The allocator's `unsafe` is
//! encapsulated in the dependency (see CLAUDE.md section 8).
//!
//! Run with: cargo bench --bench mem

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use peak_alloc::PeakAlloc;
use three_mem_fast::{Scene3mfBuilder, open};

#[global_allocator]
static PEAK: PeakAlloc = PeakAlloc;

/// Runs `f`, measures the peak heap reached during its execution and prints it.
/// The result is kept alive until after reading the peak (the peak happens while
/// the parsed model and any intermediate buffers coexist).
fn measure<T>(name: &str, f: impl FnOnce() -> T) {
    PEAK.reset_peak_usage();
    let base = PEAK.current_usage_as_mb();
    let result = f();
    let peak = PEAK.peak_usage_as_mb();
    println!(
        "{name:<18} peak heap = {peak:>7.1} MB   (parse footprint = {:>7.1} MB)",
        peak - base
    );
    drop(result); // free before the next measurement
}

fn main() {
    let path = Path::new("fixtures/bench/Snakeman.3mf");
    let size_mb = std::fs::metadata(path).unwrap().len() as f64 / 1_048_576.0;
    println!("File: {} ({size_mb:.0} MB compressed)\n", path.display());

    // three-mem-fast: streaming, geometry only, f32 coords.
    measure("three-mem-fast", || {
        let mut parser = open(path).unwrap();
        let mut builder = Scene3mfBuilder::default();
        parser.parse_root_part(&mut builder).unwrap();
        builder.into_scene()
    });

    // threemf: serde, materializes the whole model, f64 coords.
    measure("threemf", || {
        let reader = BufReader::new(File::open(path).unwrap());
        threemf::read(reader).unwrap()
    });

    // lib3mf: pure Rust, full spec.
    measure("lib3mf", || {
        let file = File::open(path).unwrap();
        lib3mf::Model::from_reader(file).unwrap()
    });

    // lib3mf-core: materializes the decompressed .model into a Vec<u8> before parsing.
    measure("lib3mf-core", || {
        use lib3mf_core::archive::{ArchiveReader, ZipArchiver, find_model_path};
        use lib3mf_core::parser::parse_model;
        let file = File::open(path).unwrap();
        let mut archiver = ZipArchiver::new(file).unwrap();
        let model_path = find_model_path(&mut archiver).unwrap();
        let model_data = archiver.read_entry(&model_path).unwrap();
        parse_model(std::io::Cursor::new(model_data)).unwrap()
    });
}
