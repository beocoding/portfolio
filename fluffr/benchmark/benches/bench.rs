use std::time::Duration;

// benches/bench.rs
use benchmark::{
    FlatrProductData, FlatrProductDataRegistry,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fluffr::prelude::*;

// ── Product helpers ───────────────────────────────────────────────────────────

fn make_flatr_registry(n: usize) -> FlatrProductDataRegistry {
    let rows: Vec<FlatrProductData> = (0..n).map(FlatrProductData::sample).collect();
    FlatrProductDataRegistry {
        sku:         rows.iter().map(|r| r.sku.clone()).collect(),
        label:       rows.iter().map(|r| r.label.clone()).collect(),
        slug:        rows.iter().map(|r| r.slug.clone()).collect(),
        description: rows.iter().map(|r| r.description.clone()).collect(),
        brand:       rows.iter().map(|r| r.brand.clone()).collect(),
        tags:        rows.iter().map(|r| r.tags.clone()).collect(),
        category:    rows.iter().map(|r| r.category).collect(),
        price:       rows.iter().map(|r| r.price).collect(),
        weight:      rows.iter().map(|r| r.weight).collect(),
        dimensions:  rows.iter().map(|r| r.dimensions).collect(),
        link:        rows.iter().map(|r| r.link.clone()).collect(),
    }
}

// ── Product benchmarks (Fluffr Only) ──────────────────────────────────────────

fn bench_product_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("product_encode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(10));
    let i = 42;

    let flatr_val = FlatrProductData::sample(i);

    group.throughput(Throughput::Bytes(flatr_val.as_buffer().bytes().len() as u64));
    group.bench_function("fluffr", |b| b.iter(|| flatr_val.as_buffer().bytes().to_vec()));

    group.finish();
}

fn bench_product_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("product_decode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(10));
    let i = 42;

    let flatr_bytes = FlatrProductData::sample(i).as_buffer().bytes().to_vec();

    group.throughput(Throughput::Bytes(flatr_bytes.len() as u64));
    group.bench_function("fluffr", |b| b.iter_batched(
        || flatr_bytes.as_slice(),
        |bytes| {
            let root = read_root(bytes) as usize;
            std::hint::black_box(FlatrProductData::view(bytes, root))
        },
        criterion::BatchSize::SmallInput,
    ));

    group.finish();
}

fn bench_product_network_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("product_network_latency");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(10));
    let i = 42;

    let flatr_bytes = FlatrProductData::sample(i).as_buffer().bytes().to_vec();

    group.throughput(Throughput::Bytes(flatr_bytes.len() as u64));
    group.bench_function("fluffr", |b| b.iter_batched(
        || flatr_bytes.as_slice(),
        |bytes| {
            let root = read_root(bytes) as usize;
            let v = FlatrProductData::view(bytes, root);
            std::hint::black_box(v.sku());
            std::hint::black_box(v.label());
            std::hint::black_box(v.slug());
            std::hint::black_box(v.description());
            std::hint::black_box(v.brand());
            std::hint::black_box(v.tags());
            std::hint::black_box(v.category());
            std::hint::black_box(v.price());
            std::hint::black_box(v.weight());
            std::hint::black_box(v.dimensions());
            std::hint::black_box(v.link());
        },
        criterion::BatchSize::SmallInput,
    ));

    group.finish();
}

// ── Registry benchmarks (Fluffr Only) ─────────────────────────────────────────
static PRODUCT_COUNTS: [usize; 4] = [100usize, 500, 1000, 10_000];

fn bench_registry_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_encode");

    for n in PRODUCT_COUNTS {
        if n >= 10_000 {
            group.sample_size(50);
            group.measurement_time(Duration::from_secs(20));
        } else {
            group.sample_size(200);
            group.measurement_time(Duration::from_secs(10));
        }
        let flatr_reg = make_flatr_registry(n);
        let encoded = flatr_reg.as_buffer().bytes().to_vec();

        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_function(BenchmarkId::new("fluffr", n),
            |b| b.iter(|| flatr_reg.as_buffer().bytes().to_vec()));
    }

    group.finish();
}

fn bench_registry_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_decode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(15));

    for n in PRODUCT_COUNTS {
        let flatr_bytes = make_flatr_registry(n).as_buffer().bytes().to_vec();

        group.throughput(Throughput::Bytes(flatr_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("fluffr", n), |b| b.iter_batched(
            || flatr_bytes.as_slice(),
            |bytes| {
                let root = read_root(bytes) as usize;
                std::hint::black_box(FlatrProductDataRegistry::view(bytes, root))
            },
            criterion::BatchSize::SmallInput,
        ));
    }

    group.finish();
}

fn bench_registry_network_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_network_latency");

    for n in PRODUCT_COUNTS {
        if n >= 10_000 {
            group.sample_size(50);
            group.measurement_time(Duration::from_secs(20));
        } else {
            group.sample_size(200);
            group.measurement_time(Duration::from_secs(10));
        }
        let flatr_bytes = make_flatr_registry(n).as_buffer().bytes().to_vec();

        group.throughput(Throughput::Bytes(flatr_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("fluffr", n), |b| b.iter_batched(
            || flatr_bytes.as_slice(),
            |bytes| {
                let root = read_root(bytes) as usize;
                let v = FlatrProductDataRegistry::view(bytes, root);
                let skus       = v.sku();
                let labels     = v.label();
                let slugs      = v.slug();
                let descs      = v.description();
                let brands     = v.brand();
                let tags       = v.tags();
                let categories = v.category();
                let prices     = v.price();
                let weights    = v.weight();
                let dimensions = v.dimensions();
                let links      = v.link();
                for i in 0..v.len() {
                    std::hint::black_box(skus.get(i));
                    std::hint::black_box(labels.get(i));
                    std::hint::black_box(slugs.get(i));
                    std::hint::black_box(descs.get(i));
                    std::hint::black_box(brands.get(i));
                    std::hint::black_box(tags.get(i));
                    std::hint::black_box(categories.get(i));
                    std::hint::black_box(prices.get(i));
                    std::hint::black_box(weights.get(i));
                    std::hint::black_box(dimensions.get(i));
                    std::hint::black_box(links.get(i));
                }
            },
            criterion::BatchSize::SmallInput,
        ));
    }

    group.finish();
}

// ── Registration ──────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_product_encode,
    bench_product_decode,
    bench_product_network_latency,
    bench_registry_encode,
    bench_registry_decode,
    bench_registry_network_latency,
);

criterion_main!(benches);