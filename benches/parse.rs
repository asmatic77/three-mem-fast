use std::fs::File;
use std::hint::black_box;
use std::io::BufReader;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use three_mem_fast::{Scene3mfBuilder, open};

fn bench_parse(c: &mut Criterion) {
    let path = Path::new("fixtures/bench/Snakeman_low.3mf");

    // Pre-parse (outside the measured region) just to learn the vertex count,
    // used as the throughput divisor (Mverts/s).
    let num_vertices = {
        let mut parser = open(path).unwrap();
        let mut builder = Scene3mfBuilder::default();
        parser.parse_root_part(&mut builder).unwrap();
        let scene = builder.into_scene();
        scene
            .objects
            .values()
            .map(|o| o.mesh.vertices.len())
            .sum::<usize>() as u64
    };
    // Compressed file size, to report MB/s for the "open only" bench.
    let file_size = std::fs::metadata(path).unwrap().len();

    // DECOMPRESSED XML size of the model parts (.model): sum of the uncompressed
    // size of those ZIP entries. This is the primary metric (MB/s of XML) and the
    // honest yardstick for judging whether we are fast.
    let xml_bytes = {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).unwrap();
        let mut total = 0u64;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name().ends_with(".model") {
                total += entry.size();
            }
        }
        total
    };

    eprintln!(
        "[fixture] compressed = {:.1} MB | decompressed XML (.model) = {:.1} MB | ratio = {:.1}x | vertices = {}",
        file_size as f64 / 1_048_576.0,
        xml_bytes as f64 / 1_048_576.0,
        xml_bytes as f64 / file_size as f64,
        num_vertices
    );

    let mut group = c.benchmark_group("parse");

    // --- Benchmarks that produce geometry: throughput in bytes of decompressed XML (MB/s) ---
    group.throughput(Throughput::Bytes(xml_bytes));

    // 1) end-to-end: open + parse (total cost of loading the 3mf)
    group.bench_function("end_to_end", |b| {
        b.iter(|| {
            let mut parser = open(black_box(path)).unwrap();
            let mut builder = Scene3mfBuilder::default();
            parser.parse_root_part(&mut builder).unwrap();
            black_box(builder.into_scene())
        });
    });

    // 3) parse only: open once OUTSIDE the measured loop
    group.bench_function("parse_only", |b| {
        let mut parser = open(path).unwrap();
        b.iter(|| {
            let mut builder = Scene3mfBuilder::default();
            parser.parse_root_part(&mut builder).unwrap();
            black_box(builder.into_scene())
        });
    });

    // 4) inflate only: read the model entry (DEFLATE inflation) discarding the
    //    bytes, WITHOUT quick-xml or parsing. Isolates the pure decompression cost.
    let model_name = {
        let file = File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(BufReader::new(file)).unwrap();
        let mut name = None;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name().ends_with(".model") {
                name = Some(entry.name().to_string());
                break;
            }
        }
        name.unwrap()
    };
    group.bench_function("inflate_only", |b| {
        let file = File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(BufReader::new(file)).unwrap();
        b.iter(|| {
            let mut entry = archive.by_name(&model_name).unwrap();
            black_box(std::io::copy(&mut entry, &mut std::io::sink()).unwrap())
        });
    });

    // Cross-crate comparison benches (B6). Gated behind the `compare` feature so
    // normal `cargo bench --bench parse` stays cheap (these pull nalgebra, parry3d...).
    // Run them with: cargo bench --bench parse --features compare
    #[cfg(feature = "compare")]
    {
        // 5) `threemf` crate (serde, materializes the whole model).
        //    Equivalent to our end_to_end: open + inflate + parse into memory.
        group.bench_function("threemf_crate", |b| {
            b.iter(|| {
                let file = File::open(black_box(path)).unwrap();
                let reader = BufReader::new(file);
                black_box(threemf::read(reader).unwrap())
            });
        });

        // 6) `lib3mf` crate (pure Rust, v0.1.6, young / "vibe-coded").
        group.bench_function("lib3mf_crate", |b| {
            b.iter(|| {
                let file = File::open(black_box(path)).unwrap();
                black_box(lib3mf::Model::from_reader(file).unwrap())
            });
        });

        // 7) `lib3mf-core` crate (pure Rust, materializes the .model into a Vec).
        group.bench_function("lib3mf_core_crate", |b| {
            use lib3mf_core::archive::{ArchiveReader, ZipArchiver, find_model_path};
            use lib3mf_core::parser::parse_model;
            b.iter(|| {
                let file = File::open(black_box(path)).unwrap();
                let mut archiver = ZipArchiver::new(file).unwrap();
                let model_path = find_model_path(&mut archiver).unwrap();
                let model_data = archiver.read_entry(&model_path).unwrap();
                black_box(parse_model(std::io::Cursor::new(model_data)).unwrap())
            });
        });
    }

    // --- Benchmark that does NOT produce geometry: throughput in zip bytes ---
    group.throughput(Throughput::Bytes(file_size));

    // 2) open only: File::open + ZipArchive + _rels/.rels + [Content_Types].xml
    group.bench_function("open_only", |b| {
        b.iter(|| black_box(open(black_box(path)).unwrap()));
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
