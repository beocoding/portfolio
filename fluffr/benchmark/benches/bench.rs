use std::time::Duration;

// benches/bench.rs
use benchmark::{
    // event (types still needed for helpers even though benches are disabled)
    Event, EventArgs, FlatrEvent, JsonEvent, JsonEventRef, ProtoEvent,
    // product
    FlatrProductData, FlatrProductDataRegistry,
    JsonProductData,
    ProtoProductData, ProtoProductRegistry,
    // fbs product
    FbsBrand, FbsBrandArgs, FbsCategory,
    FbsDimensions,
    FbsLinkBySku, FbsLinkBySkuArgs,
    FbsLinkByDims, FbsLinkByDimsArgs,
    FbsProduct, FbsProductArgs,
    FbsProductLink,
    FbsProductRegistry, FbsProductRegistryArgs,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use flatbuffers::FlatBufferBuilder;
use prost::Message;
use fluffr::prelude::*;

// ── Event helpers (kept for completeness) ─────────────────────────────────────

fn make_json_bytes(data_len: usize) -> Vec<u8> {
    serde_json::to_vec(&JsonEvent::sample(data_len)).unwrap()
}

fn make_proto_bytes(data_len: usize) -> Vec<u8> {
    ProtoEvent::from(&JsonEvent::sample(data_len)).encode_to_vec()
}

fn make_fb_bytes(data_len: usize) -> Vec<u8> {
    let e = JsonEvent::sample(data_len);
    let mut b = FlatBufferBuilder::with_capacity(256);
    let t       = b.create_string(&e.r#type);
    let subject = b.create_string(&e.subject);
    let source  = b.create_string(&e.source);
    let time    = b.create_string(&e.time);
    let data    = b.create_string(&e.data);
    let event = Event::create(&mut b, &EventArgs {
        type_: Some(t), subject: Some(subject),
        source: Some(source), time: Some(time), data: Some(data),
    });
    b.finish(event, None);
    b.finished_data().to_vec()
}

fn make_flatr_bytes(data_len: usize) -> Vec<u8> {
    FlatrEvent::sample(data_len).as_buffer().bytes().to_vec()
}

// ── Product helpers ───────────────────────────────────────────────────────────

fn fbs_category(i: usize) -> FbsCategory {
    match i % 5 {
        0 => FbsCategory::Electronics,
        1 => FbsCategory::Apparel,
        2 => FbsCategory::Home,
        3 => FbsCategory::Grocery,
        _ => FbsCategory::Other,
    }
}

fn make_fbs_link(
    b: &mut FlatBufferBuilder,
    i: usize,
) -> (FbsProductLink, flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>) {
    match i % 3 {
        0 => {
            let s = b.create_string(&format!("REF-{}", i));
            let l = FbsLinkBySku::create(b, &FbsLinkBySkuArgs { sku: Some(s) });
            (FbsProductLink::LinkBySku, l.as_union_value())
        }
        1 => {
            let l = FbsLinkByDims::create(b, &FbsLinkByDimsArgs {
                dims: Some(&FbsDimensions::new(i as f32, 1.0, 1.0)),
            });
            (FbsProductLink::LinkByDims, l.as_union_value())
        }
        _ => {
            let s = b.create_string(&format!("Linked {}", i));
            let l = FbsBrand::create(b, &FbsBrandArgs { name: Some(s) });
            (FbsProductLink::Brand, l.as_union_value())
        }
    }
}

fn make_fbs_product_bytes(i: usize) -> Vec<u8> {
    let mut b = FlatBufferBuilder::with_capacity(512);
    let sku         = b.create_string(&format!("SKU-{:06}", i));
    let label       = b.create_string(&format!("Product Label {}", i));
    let slug        = b.create_string(&format!("product-label-{}", i));
    let description = b.create_string(&format!("Description for product {}", i));
    let brand_name  = b.create_string(&format!("Brand {}", i % 5));
    let tag0        = b.create_string(&format!("tag-{}", i % 3));
    let tag1        = b.create_string("common");
    let tags        = b.create_vector(&[tag0, tag1]);
    let brand       = FbsBrand::create(&mut b, &FbsBrandArgs { name: Some(brand_name) });
    let dims        = FbsDimensions::new(10.0 + i as f32, 5.0, 2.0);
    let (link_type, link) = make_fbs_link(&mut b, i);
    let product = FbsProduct::create(&mut b, &FbsProductArgs {
        sku: Some(sku), label: Some(label), slug: Some(slug),
        description: Some(description), brand: Some(brand),
        tags: Some(tags), category: fbs_category(i),
        price: 9.99 + i as f32, weight: 0.5 + i as f32 * 0.1,
        dimensions: Some(&dims), link_type, link: Some(link),
    });
    b.finish(product, None);
    b.finished_data().to_vec()
}

fn make_fbs_registry_bytes(n: usize) -> Vec<u8> {
    let mut b = FlatBufferBuilder::with_capacity(512 * n);
    let products: Vec<_> = (0..n).map(|i| {
        let sku         = b.create_string(&format!("SKU-{:06}", i));
        let label       = b.create_string(&format!("Product Label {}", i));
        let slug        = b.create_string(&format!("product-label-{}", i));
        let description = b.create_string(&format!("Description for product {}", i));
        let brand_name  = b.create_string(&format!("Brand {}", i % 5));
        let tag0        = b.create_string(&format!("tag-{}", i % 3));
        let tag1        = b.create_string("common");
        let tags        = b.create_vector(&[tag0, tag1]);
        let brand       = FbsBrand::create(&mut b, &FbsBrandArgs { name: Some(brand_name) });
        let dims        = FbsDimensions::new(10.0 + i as f32, 5.0, 2.0);
        let (link_type, link) = make_fbs_link(&mut b, i);
        FbsProduct::create(&mut b, &FbsProductArgs {
            sku: Some(sku), label: Some(label), slug: Some(slug),
            description: Some(description), brand: Some(brand),
            tags: Some(tags), category: fbs_category(i),
            price: 9.99 + i as f32, weight: 0.5 + i as f32 * 0.1,
            dimensions: Some(&dims), link_type, link: Some(link),
        })
    }).collect();
    let products_vec = b.create_vector(&products);
    let registry = FbsProductRegistry::create(&mut b, &FbsProductRegistryArgs {
        products: Some(products_vec),
    });
    b.finish(registry, None);
    b.finished_data().to_vec()
}

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

// ── Event benchmarks (disabled) ───────────────────────────────────────────────

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");
    for data_len in [10usize, 500, 1_000, 10_000] {
        let json_val  = JsonEvent::sample(data_len);
        let proto_val = ProtoEvent::from(&json_val);
        let flatr_val = FlatrEvent::sample(data_len);

        group.throughput(Throughput::Bytes(make_json_bytes(data_len).len() as u64));
        group.bench_function(BenchmarkId::new("json",    data_len),
            |b| b.iter(|| serde_json::to_vec(&json_val).unwrap()));
        group.throughput(Throughput::Bytes(make_proto_bytes(data_len).len() as u64));
        group.bench_function(BenchmarkId::new("proto",   data_len),
            |b| b.iter(|| proto_val.encode_to_vec()));
        group.throughput(Throughput::Bytes(make_fb_bytes(data_len).len() as u64));
        group.bench_function(BenchmarkId::new("flatbuf", data_len),
            |b| b.iter(|| make_fb_bytes(data_len)));
        group.throughput(Throughput::Bytes(make_flatr_bytes(data_len).len() as u64));
        group.bench_function(BenchmarkId::new("fluffr",   data_len),
            |b| b.iter(|| flatr_val.as_buffer().bytes().to_vec()));
    }
    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");
    for data_len in [10usize, 500, 1_000, 10_000] {
        let json_bytes  = make_json_bytes(data_len);
        let proto_bytes = make_proto_bytes(data_len);
        let fb_bytes    = make_fb_bytes(data_len);
        let flatr_bytes = make_flatr_bytes(data_len);

        group.throughput(Throughput::Bytes(json_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("json",    data_len),
            |b| b.iter(|| serde_json::from_slice::<JsonEvent>(&json_bytes).unwrap()));
        group.throughput(Throughput::Bytes(proto_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("proto",   data_len),
            |b| b.iter(|| ProtoEvent::decode(proto_bytes.as_slice()).unwrap()));
        group.throughput(Throughput::Bytes(fb_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("flatbuf", data_len),
            |b| b.iter(|| std::hint::black_box(flatbuffers::root::<Event>(&fb_bytes).unwrap())));
        group.throughput(Throughput::Bytes(flatr_bytes.len() as u64));
        group.bench_function(BenchmarkId::new("fluffr",   data_len),
            |b| b.iter(|| {
                let root = read_root(&flatr_bytes) as usize;
                std::hint::black_box(FlatrEvent::view(&flatr_bytes, root))
            }));
    }
    group.finish();
}

fn bench_read_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_latency");
    for data_len in [10usize, 500, 1_000, 10_000] {
        let json_bytes  = make_json_bytes(data_len);
        let proto_bytes = make_proto_bytes(data_len);
        let fb_bytes    = make_fb_bytes(data_len);
        let flatr_bytes = make_flatr_bytes(data_len);

        let json_decoded  = serde_json::from_slice::<JsonEvent>(&json_bytes).unwrap();
        let proto_decoded = ProtoEvent::decode(proto_bytes.as_slice()).unwrap();
        let fb_view       = flatbuffers::root::<Event>(&fb_bytes).unwrap();
        let flatr_root    = read_root(&flatr_bytes) as usize;
        let flatr_view    = FlatrEvent::view(&flatr_bytes, flatr_root);

        group.bench_function(BenchmarkId::new("json",    data_len),
            |b| b.iter(|| std::hint::black_box(&json_decoded.data)));
        group.bench_function(BenchmarkId::new("proto",   data_len),
            |b| b.iter(|| std::hint::black_box(&proto_decoded.data)));
        group.bench_function(BenchmarkId::new("flatbuf", data_len),
            |b| b.iter(|| std::hint::black_box(fb_view.data())));
        group.bench_function(BenchmarkId::new("fluffr",   data_len),
            |b| b.iter(|| std::hint::black_box(flatr_view.data())));
    }
    group.finish();
}

fn bench_network_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_latency");
    for data_len in [10usize, 500, 1_000, 10_000] {
        let json_bytes  = make_json_bytes(data_len);
        let proto_bytes = make_proto_bytes(data_len);
        let fb_bytes    = make_fb_bytes(data_len);
        let flatr_bytes = make_flatr_bytes(data_len);

        group.bench_function(BenchmarkId::new("json",    data_len), |b| b.iter(|| {
            let e = serde_json::from_slice::<JsonEventRef>(&json_bytes).unwrap();
            std::hint::black_box(e.data)
        }));
        group.bench_function(BenchmarkId::new("proto",   data_len), |b| b.iter(|| {
            let e = ProtoEvent::decode(proto_bytes.as_slice()).unwrap();
            std::hint::black_box(e.data)
        }));
        group.bench_function(BenchmarkId::new("flatbuf", data_len), |b| b.iter(|| {
            let e = flatbuffers::root::<Event>(&fb_bytes).unwrap();
            std::hint::black_box(e.data())
        }));
        group.bench_function(BenchmarkId::new("fluffr",   data_len), |b| b.iter(|| {
            let root = read_root(&flatr_bytes) as usize;
            let view = FlatrEvent::view(&flatr_bytes, root);
            std::hint::black_box(view.data())
        }));
    }
    group.finish();
}
// ── Product benchmarks ────────────────────────────────────────────────────────

fn bench_product_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("product_encode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(10));
    let i = 42;

    let json_val  = JsonProductData::sample(i);
    let proto_val = ProtoProductData::sample(i);
    let flatr_val = FlatrProductData::sample(i);

    group.bench_function("json",    |b| b.iter(|| serde_json::to_vec(&json_val).unwrap()));
    group.bench_function("proto",   |b| b.iter(|| proto_val.encode_to_vec()));
    group.bench_function("flatbuf", |b| b.iter(|| make_fbs_product_bytes(i)));
    group.bench_function("fluffr",   |b| b.iter(|| flatr_val.as_buffer().bytes().to_vec()));

    group.finish();
}

fn bench_product_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("product_decode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(10));
    let i = 42;

    let json_bytes  = serde_json::to_vec(&JsonProductData::sample(i)).unwrap();
    let proto_bytes = ProtoProductData::sample(i).encode_to_vec();
    let fbs_bytes   = make_fbs_product_bytes(i);
    let flatr_bytes = FlatrProductData::sample(i).as_buffer().bytes().to_vec();

    group.bench_function("json",    |b| b.iter(||
        serde_json::from_slice::<JsonProductData>(&json_bytes).unwrap()));
    group.bench_function("proto",   |b| b.iter(||
        ProtoProductData::decode(proto_bytes.as_slice()).unwrap()));
    group.bench_function("flatbuf", |b| b.iter(||
        std::hint::black_box(flatbuffers::root::<FbsProduct>(&fbs_bytes).unwrap())));
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

    let json_bytes  = serde_json::to_vec(&JsonProductData::sample(i)).unwrap();
    let proto_bytes = ProtoProductData::sample(i).encode_to_vec();
    let fbs_bytes   = make_fbs_product_bytes(i);
    let flatr_bytes = FlatrProductData::sample(i).as_buffer().bytes().to_vec();

    group.bench_function("json", |b| b.iter(|| {
        let p = serde_json::from_slice::<JsonProductData>(&json_bytes).unwrap();
        std::hint::black_box(&p.sku);
        std::hint::black_box(&p.label);
        std::hint::black_box(&p.slug);
        std::hint::black_box(&p.description);
        std::hint::black_box(&p.brand.name);
        std::hint::black_box(&p.tags);
        std::hint::black_box(p.category);
        std::hint::black_box(p.price);
        std::hint::black_box(p.weight);
        std::hint::black_box(&p.dimensions);
        std::hint::black_box(&p.link);
    }));

    group.bench_function("proto", |b| b.iter(|| {
        let p = ProtoProductData::decode(proto_bytes.as_slice()).unwrap();
        std::hint::black_box(&p.sku);
        std::hint::black_box(&p.label);
        std::hint::black_box(&p.slug);
        std::hint::black_box(&p.description);
        std::hint::black_box(&p.brand);
        std::hint::black_box(&p.tags);
        std::hint::black_box(p.category);
        std::hint::black_box(p.price);
        std::hint::black_box(p.weight);
        std::hint::black_box(&p.dimensions);
        std::hint::black_box(&p.link);
    }));

    group.bench_function("flatbuf", |b| b.iter(|| {
        let p = flatbuffers::root::<FbsProduct>(&fbs_bytes).unwrap();
        std::hint::black_box(p.sku());
        std::hint::black_box(p.label());
        std::hint::black_box(p.slug());
        std::hint::black_box(p.description());
        std::hint::black_box(p.brand());
        std::hint::black_box(p.tags());
        std::hint::black_box(p.category());
        std::hint::black_box(p.price());
        std::hint::black_box(p.weight());
        std::hint::black_box(p.dimensions());
        std::hint::black_box(p.link_type());
    }));

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

// ── Registry benchmarks ───────────────────────────────────────────────────────
static PRODUCT_COUNTS: [usize;4] = [100usize, 500, 1000, 10_000];
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
        let json_rows = JsonProductData::sample_registry(n);
        let proto_reg = ProtoProductData::sample_registry(n);
        let flatr_reg = make_flatr_registry(n);

        group.bench_function(BenchmarkId::new("json",    n),
            |b| b.iter(|| serde_json::to_vec(&json_rows).unwrap()));
        group.bench_function(BenchmarkId::new("proto",   n),
            |b| b.iter(|| proto_reg.encode_to_vec()));
        group.bench_function(BenchmarkId::new("flatbuf", n),
            |b| b.iter(|| make_fbs_registry_bytes(n)));
        group.bench_function(BenchmarkId::new("fluffr",   n),
            |b| b.iter(|| flatr_reg.as_buffer().bytes().to_vec()));
    }

    group.finish();
}

fn bench_registry_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_decode");
    group.sample_size(500);
    group.measurement_time(Duration::from_secs(15));

    for n in PRODUCT_COUNTS {
        let json_bytes  = serde_json::to_vec(&JsonProductData::sample_registry(n)).unwrap();
        let proto_bytes = ProtoProductData::sample_registry(n).encode_to_vec();
        let fbs_bytes   = make_fbs_registry_bytes(n);
        let flatr_bytes = make_flatr_registry(n).as_buffer().bytes().to_vec();

        group.bench_function(BenchmarkId::new("json",    n), |b| b.iter(||
            serde_json::from_slice::<Vec<JsonProductData>>(&json_bytes).unwrap()));
        group.bench_function(BenchmarkId::new("proto",   n), |b| b.iter(||
            ProtoProductRegistry::decode(proto_bytes.as_slice()).unwrap()));
        group.bench_function(BenchmarkId::new("flatbuf", n), |b| b.iter(||
            std::hint::black_box(flatbuffers::root::<FbsProductRegistry>(&fbs_bytes).unwrap())));
        // iter_batched to reduce harness overhead relative to the ~1-3ns body
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
        let json_bytes  = serde_json::to_vec(&JsonProductData::sample_registry(n)).unwrap();
        let proto_bytes = ProtoProductData::sample_registry(n).encode_to_vec();
        let fbs_bytes   = make_fbs_registry_bytes(n);
        let flatr_bytes = make_flatr_registry(n).as_buffer().bytes().to_vec();

        group.bench_function(BenchmarkId::new("json", n), |b| b.iter(|| {
            let rows = serde_json::from_slice::<Vec<JsonProductData>>(&json_bytes).unwrap();
            for p in &rows {
                std::hint::black_box(&p.sku);
                std::hint::black_box(&p.label);
                std::hint::black_box(&p.slug);
                std::hint::black_box(&p.description);
                std::hint::black_box(&p.brand.name);
                std::hint::black_box(&p.tags);
                std::hint::black_box(p.category);
                std::hint::black_box(p.price);
                std::hint::black_box(p.weight);
                std::hint::black_box(&p.dimensions);
                std::hint::black_box(&p.link);
            }
        }));

        group.bench_function(BenchmarkId::new("proto", n), |b| b.iter(|| {
            let reg = ProtoProductRegistry::decode(proto_bytes.as_slice()).unwrap();
            for p in &reg.products {
                std::hint::black_box(&p.sku);
                std::hint::black_box(&p.label);
                std::hint::black_box(&p.slug);
                std::hint::black_box(&p.description);
                std::hint::black_box(&p.brand);
                std::hint::black_box(&p.tags);
                std::hint::black_box(p.category);
                std::hint::black_box(p.price);
                std::hint::black_box(p.weight);
                std::hint::black_box(&p.dimensions);
                std::hint::black_box(&p.link);
            }
        }));

        group.bench_function(BenchmarkId::new("flatbuf", n), |b| b.iter(|| {
            let reg = flatbuffers::root::<FbsProductRegistry>(&fbs_bytes).unwrap();
            let products = reg.products().unwrap_or_default();
            for i in 0..products.len() {
                let p = products.get(i);
                std::hint::black_box(p.sku());
                std::hint::black_box(p.label());
                std::hint::black_box(p.slug());
                std::hint::black_box(p.description());
                std::hint::black_box(p.brand());
                std::hint::black_box(p.tags());
                std::hint::black_box(p.category());
                std::hint::black_box(p.price());
                std::hint::black_box(p.weight());
                std::hint::black_box(p.dimensions());
                std::hint::black_box(p.link_type());
            }
        }));

        // fluffr: columns are contiguous arrays — each field is one ListView scan
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
// Event benches disabled — uncomment to re-enable:
// bench_encode, bench_decode, bench_read_latency, bench_network_latency,

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