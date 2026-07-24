# Benchmarks

Performance log for the parser. From milestone H4 onward, every optimization
requires a measured before/after (criterion, `cargo bench --bench parse`).

## Environment

- **Machine:** 12th Gen Intel Core i7-12700H
- **OS:** Windows 11
- **Toolchain:** rustc 1.96.1, `bench` profile (optimized)
- **Tool:** criterion 0.8, `--sample-size 10`
- Always run plugged in and on a high-performance power plan (on battery, Windows
  caps CPU frequency and all numbers come out ~15% slower).

## Fixtures

- `fixtures/bench/Snakeman_low.3mf` — 9.5 MB compressed → **47.2 MB of decompressed XML** (ratio 5.0x), **300,241 vertices**.
- `fixtures/bench/Snakeman.3mf` — 81 MB compressed (~400 MB decompressed XML). Used for the memory comparison.

## Primary metric (2026-07-24)

With `fast-float2` + `atoi`, `parse_only` = **~375 ms → ~125 MiB/s of decompressed XML**.
Single-threaded pipeline: DEFLATE inflation + quick-xml tokenizing + number parsing + geometry building.

### Per-stage breakdown (`inflate_only` vs `parse_only`)

| Stage | Time | % | XML MB/s |
|-------|------|---|----------|
| `inflate_only` (DEFLATE only, discarding bytes) | ~60 ms | ~15% | ~780 MiB/s |
| `parse_only` (inflate + quick-xml + parsing) | ~400 ms | 100% | ~117 MiB/s |
| **→ quick-xml + own parsing (by difference)** | **~340 ms** | **~85%** | — |

**Key finding: DEFLATE inflation is NOT the bottleneck (only 15%).** flate2/zlib-rs is plenty fast (~780 MiB/s).
The **85%** of the time is spent **tokenizing XML (quick-xml) + own parsing**. `find_vertex`/`find_triangle` are
already well optimized, so a large part of that 85% is probably quick-xml producing events (`read_event_into`).

**Next:** separate pure quick-xml from own parsing (a "tokenize only" bench: iterate events without calling the
`find_*` functions). If quick-xml dominates, levers: `Reader` config (`check_end_names`, etc.), `BufReader` size (B2).

## Per-stage breakdown (initial baseline)

| Bench | Time | Notes |
|-------|------|-------|
| `open_only` | ~48 µs | File::open + ZipArchive + `_rels/.rels` + `[Content_Types].xml`. Negligible. |
| `parse_only` | ~565 ms | `parse_root_part` only. 99.99% of the cost is here. |
| `end_to_end` | ~563 ms | `open` + `parse`. Practically identical to `parse_only`. |

Conclusion: the bottleneck is entirely in geometry parsing, not in opening the container.

## Change history (bench: `parse_only`, Snakeman_low)

| Date | Change | Time | Δ vs previous | Notes |
|------|--------|------|---------------|-------|
| 2026-07-23 | Baseline: `parse_vertex` with 3x `find_attr_value` (3 attribute scans per vertex) | ~565 ms | — | starting point |
| 2026-07-23 | `find_vertex`: single scan of `attributes()`, parse inside the `match` (D10) | ~537 ms | **−5.0%** (p=0.00) | smaller impact than expected → redundant scans were not the bottleneck |
| 2026-07-23 | Experiment `from_utf8_unchecked` on v+t (unsafe, NOT committed, reverted) | ~447 ms | **−9.0%** (p=0.00) | with BOTH paths covered, skipping UTF-8 gives 9% (not ~2% as measuring vertices alone suggested). **Corrects the earlier conclusion.** |
| 2026-07-23 | `find_triangle`: same single-scan pattern for triangles (v1/v2/v3, `u32`). All safe. | ~492 ms | **−6.6%** (p=0.00) | bigger jump than vertices because a closed mesh has ~2x more triangles than vertices. |
| 2026-07-24 | `fast-float2::parse` from `&[u8]` in `find_vertex` (vertices only; triangles still `str::parse::<u32>`). Safe, committable. | ~384 ms | **−14%** (p=0.00) | more than predicted by the UTF-8-unsafe experiment (~9% for v+t). → `fast-float2` doesn't just avoid UTF-8 validation: **its float parsing algorithm is itself faster than `str::parse::<f32>`**. |
| 2026-07-24 | `atoi::atoi::<u32>` from `&[u8]` in `find_triangle` (integers v1/v2/v3). Safe. | ~375 ms | **−2.3%** (p=0.00) | far less than the −14% for floats, despite ~2x more triangles. → integer parsing was already cheap in std (`str::parse::<u32>` is a simple loop); the bulk of the float gain came from the expensive `str::parse::<f32>` algorithm, not just UTF-8. |

**Clean measurement (all safe, v+t optimized):** baseline 565 ms → 375 ms = **−34% total**.

**B1 conclusion:** `fast-float2` wins in TWO ways, not one: (1) parsing from `&[u8]` without UTF-8 validation, (2) a
faster parsing algorithm than std. The CLAUDE.md hypothesis ("modest gain, only from UTF-8") is refuted by measurement.
**B1 resolved: adopt `fast-float2`.** Integers: `atoi` gives a small extra 2.3%. `atoi_simd` was **rejected** (below).

Method lesson: always measure the full path before extrapolating. A micro-optimization tested on half the workload
underestimates its total effect.

### SIMD experiment for integers (2026-07-24)

Note: from here on, `RUSTFLAGS="-C target-cpu=native"` was used to enable SIMD (AVX2) on the i7-12700H.

| Config | Time | vs previous | Notes |
|--------|------|-------------|-------|
| `atoi` + `target-cpu=native` | ~366 ms | −2.4% vs `atoi` without native | native flags speed up the WHOLE pipeline (quick-xml, flate2, atoi), not just integers. New baseline for the SIMD comparison. |
| `atoi_simd::parse::<u32,true,true>` + native | ~395 ms | **+8.0% (REGRESSION)** (p=0.00) | SIMD is **slower** for short indices (1-6 digits). The overhead of setting up the vector operation is not amortized on such short numbers. |

**Conclusion: reject `atoi_simd`.** SIMD wins on LONG integers, not on MANY short integers. "Numerous" ≠ "long".
Measuring avoided adopting an optimization that ran 8% slower, with an extra dependency, internal unsafe and
portability concerns (`target-cpu=native` is not portable for a library).

## Open questions still to measure

- **B2** — optimal `BufReader` / pipeline channel buffer size.
- **B5** — own parser vs `lib3mf-core`.
- Real profiling (samply / VS profiler) to see the split between: number parsing, quick-xml tokenizing, DEFLATE inflation.

## Comparison against other crates (B6, 2026-07-24)

Same file (Snakeman_low, 47.2 MB XML), end-to-end read + parse into memory. All are dev-dependencies.

### Isolated run (us vs threemf only)

| Reader | Time | XML MB/s |
|--------|------|----------|
| **`three-mem-fast`** | ~372 ms | ~127 MiB/s |
| `threemf` v0.8 | ~469 ms | ~100 MiB/s |

### Joint run (all 4 back-to-back, laptop PLUGGED IN — valid numbers)

Note: a first run came out ~15% slower across ALL readers because the laptop was **unplugged** (Windows caps CPU
frequency on battery). Re-run plugged in. Rule: always benchmark plugged in on a high-performance power plan.

| Reader | Time | XML MB/s | vs us | Parsing scope |
|--------|------|----------|-------|---------------|
| **`three-mem-fast`** | ~385 ms | ~122 MiB/s | — | geometry only, streaming (D5), `f32` |
| `lib3mf` v0.1.6 | ~447 ms | ~106 MiB/s | +16% | full spec + mesh ops; young, "vibe-coded" (AI), not an ecosystem standard |
| `threemf` v0.8 | ~467 ms | ~101 MiB/s | +21% | core geometry, serde (materializes) |
| `lib3mf-core` v0.4 | ~482 ms | ~98 MiB/s | +25% | full spec + all extensions; materializes the .model into `Vec<u8>` |

**We are the fastest of the four.** The relative ordering was identical plugged and unplugged → robust ranking.

**Honest caveats:**
1. **We parse less**: geometry only; `lib3mf`/`lib3mf-core` parse the full spec + extensions → more work. BUT geometry
   is ~95% of the bytes, so their extra work is on the small remainder. The advantage is not only "doing less". As we
   add more of the standard, the comparison will become less favorable.
2. **Streaming vs materialize**: `threemf` and `lib3mf-core` load the whole decompressed XML into RAM. On a ~1 GB 3MF
   we survive (D5); they OOM. The 47 MB bench doesn't show it.
3. **Precision**: we use `f32` (12 B/vertex, wgpu-aligned); `threemf` uses `f64` (24 B).
4. **Thermal**: for fine numbers, run each reader isolated with cooldown, or on a desktop.

**Build cost**: `lib3mf`/`lib3mf-core` pull in `nalgebra`, `parry3d`, `spade`, `clipper2`, `earcutr`, `glam`...
They heavily inflate compile time. TODO: gate the comparison behind a feature flag, or drop these dev-deps after
recording the numbers, so `cargo test` / `cargo build` are not penalized day to day.

### MEMORY comparison (peak heap, 2026-07-24)

Separate harness: `benches/mem.rs` (`cargo bench --bench mem`), global allocator `peak_alloc` tracking peak live bytes.
Large file: `fixtures/bench/Snakeman.3mf` — **81 MB compressed** (~400 MB of decompressed XML at 5x ratio).

| Reader | Peak heap | vs us | Why |
|--------|-----------|-------|-----|
| **`three-mem-fast`** | **192 MB** | — | streaming (D5): never holds the whole XML; geometry only; `f32` |
| `threemf` v0.8 | 384 MB | **2.0x** | materializes the model (serde) + `f64` (24 B/vertex, double ours) |
| `lib3mf-core` v0.4 | 1088 MB | **5.7x** | materializes the decompressed .model (~400 MB) into `Vec<u8>` + full-spec model |
| `lib3mf` v0.1.6 | 1344 MB | **7.0x** | full spec + mesh ops (`nalgebra` structures), materializes |

**Conclusion: we are the lightest on memory by a wide margin (2x–7x less).** This is the key architectural advantage
of D5 that the speed benchmark did not capture: for the 81 MB file, `lib3mf` needs **1.3 GB**; we need 192 MB.
For the project goal (3MF files up to ~1 GB where materializing readers OOM, Requirement 2), this is what really
matters. Our ~192 MB ≈ the geometry in RAM (f32 vertices + u32 triangles), without the decompressed XML.
