use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use three_mem_fast::{Scene3mfBuilder, open};

fn bench_parse(c: &mut Criterion) {
    let path = Path::new("fixtures/bench/Snakeman_low.3mf");

    // Pre-parseo (fuera de la medición) solo para conocer el nº de vértices,
    // que usamos como divisor de throughput (Mverts/s).
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
    // Tamaño comprimido del fichero, para dar MB/s al bench de "solo abrir".
    let file_size = std::fs::metadata(path).unwrap().len();

    let mut group = c.benchmark_group("parse");

    // --- Benchmarks que producen geometría: throughput en vértices ---
    group.throughput(Throughput::Elements(num_vertices));

    // 1) end-to-end: abrir + parsear (coste total de cargar el 3mf)
    group.bench_function("end_to_end", |b| {
        b.iter(|| {
            let mut parser = open(black_box(path)).unwrap();
            let mut builder = Scene3mfBuilder::default();
            parser.parse_root_part(&mut builder).unwrap();
            black_box(builder.into_scene())
        });
    });

    // 3) solo parsear: abrimos una vez FUERA del bucle medido
    group.bench_function("parse_only", |b| {
        let mut parser = open(path).unwrap();
        b.iter(|| {
            let mut builder = Scene3mfBuilder::default();
            parser.parse_root_part(&mut builder).unwrap();
            black_box(builder.into_scene())
        });
    });

    // --- Benchmark que NO produce geometría: throughput en bytes del zip ---
    group.throughput(Throughput::Bytes(file_size));

    // 2) solo abrir: File::open + ZipArchive + _rels/.rels + [Content_Types].xml
    group.bench_function("open_only", |b| {
        b.iter(|| black_box(open(black_box(path)).unwrap()));
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
