use std::fs::File;
use std::hint::black_box;
use std::io::BufReader;
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

    // Tamaño XML DESCOMPRIMIDO de las partes modelo (.model): suma de la talla
    // sin comprimir de esas entries del ZIP. Es la métrica principal (MB/s de XML)
    // que pide el CLAUDE.md, y la vara honesta para juzgar si vamos rápido o no.
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
        "[fixture] comprimido = {:.1} MB | XML descomprimido (.model) = {:.1} MB | ratio = {:.1}x | vértices = {}",
        file_size as f64 / 1_048_576.0,
        xml_bytes as f64 / 1_048_576.0,
        xml_bytes as f64 / file_size as f64,
        num_vertices
    );

    let mut group = c.benchmark_group("parse");

    // --- Benchmarks que producen geometría: throughput en bytes de XML descomprimido (MB/s) ---
    group.throughput(Throughput::Bytes(xml_bytes));

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

    // 4) solo inflar: leer la entry del modelo (inflado DEFLATE) descartando los
    //    bytes, SIN quick-xml ni parseo. Aísla el coste puro de descompresión.
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
