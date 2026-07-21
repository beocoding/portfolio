// tests/encoded_sizes.rs

use benchmark::{
    FlatrProductData, FlatrProductDataRegistry,
    JsonProductData,
    ProtoProductData, ProtoProductRegistry,
    FbsBrand, FbsBrandArgs, FbsCategory,
    FbsDimensions,
    FbsLinkBySku, FbsLinkBySkuArgs,
    FbsLinkByDims, FbsLinkByDimsArgs,
    FbsProduct, FbsProductArgs,
    FbsProductLink,
    FbsProductRegistry, FbsProductRegistryArgs,
};
use flatbuffers::FlatBufferBuilder;
use prost::Message;
use fluffr::prelude::*;

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn encoded_size_single_product() {
    let i = 42;

    let json  = serde_json::to_vec(&JsonProductData::sample(i)).unwrap();
    let proto = ProtoProductData::sample(i).encode_to_vec();
    let fbs   = make_fbs_product_bytes(i);
    let flatr = FlatrProductData::sample(i).as_buffer().bytes().to_vec();

    println!("\n=== Single ProductData (index {i}) ===");
    println!("  {:<10} {:>8} B", "json",    json.len());
    println!("  {:<10} {:>8} B", "proto",   proto.len());
    println!("  {:<10} {:>8} B", "flatbuf", fbs.len());
    println!("  {:<10} {:>8} B", "flatr",   flatr.len());
}

#[test]
fn encoded_size_registry() {
    const COUNTS: &[usize] = &[1, 10, 50, 500, 1_000, 10_000];

    println!("\n=== ProductRegistry encoded size ===");
    println!("  {:>7}  {:>10}  {:>10}  {:>10}  {:>10}",
        "n", "json", "proto", "flatbuf", "flatr");
    println!("  {:-<7}  {:-<10}  {:-<10}  {:-<10}  {:-<10}",
        "", "", "", "", "");

    for &n in COUNTS {
        let json  = serde_json::to_vec(&JsonProductData::sample_registry(n)).unwrap();
        let proto = ProtoProductData::sample_registry(n).encode_to_vec();
        let fbs   = make_fbs_registry_bytes(n);
        let flatr = make_flatr_registry(n).as_buffer().bytes().to_vec();

        println!("  {:>7}  {:>8} B  {:>8} B  {:>8} B  {:>8} B",
            n, json.len(), proto.len(), fbs.len(), flatr.len());
    }
}

#[test]
fn encoded_size_registry_per_item() {
    const COUNTS: &[usize] = &[1, 10, 50, 500, 1_000, 10_000];

    println!("\n=== ProductRegistry bytes-per-item ===");
    println!("  {:>7}  {:>10}  {:>10}  {:>10}  {:>10}",
        "n", "json", "proto", "flatbuf", "flatr");
    println!("  {:-<7}  {:-<10}  {:-<10}  {:-<10}  {:-<10}",
        "", "", "", "", "");

    for &n in COUNTS {
        let json  = serde_json::to_vec(&JsonProductData::sample_registry(n)).unwrap();
        let proto = ProtoProductData::sample_registry(n).encode_to_vec();
        let fbs   = make_fbs_registry_bytes(n);
        let flatr = make_flatr_registry(n).as_buffer().bytes().to_vec();

        println!("  {:>7}  {:>8.1} B  {:>8.1} B  {:>8.1} B  {:>8.1} B",
            n,
            json.len()  as f64 / n as f64,
            proto.len() as f64 / n as f64,
            fbs.len()   as f64 / n as f64,
            flatr.len() as f64 / n as f64,
        );
    }
}


const N: usize = 1_000_000;

fn bench_hot<T: Serialize + Copy>(name: &str, sample: T) {
    let mut buf = DefaultBuffer::default();

    // warmup — also grows the buffer once so the timed loop is steady-state
    for _ in 0..1000 {
        black_box(sample).write_to(black_box(&mut buf));
    }
    buf.reset();

    let start = Instant::now();
    for _ in 0..N {
        let slot = black_box(sample).write_to(black_box(&mut buf));
        black_box(slot);
        if buf.head() < 64 {
            buf.reset();
        }
    }
    let elapsed = start.elapsed();
    println!(
        "  {:<6} {:>9} iters in {:>12?}  ({:>6.2} ns/iter)",
        name,
        N,
        elapsed,
        elapsed.as_nanos() as f64 / N as f64
    );
}

fn bench_cold<T: Serialize + Copy>(name: &str, sample: T) {
    const COLD_N: usize = 10_000;
    let start = Instant::now();
    for _ in 0..COLD_N {
        // Minimal fresh buffer forces ensure_capacity -> grow on (near) every call.
        let mut buf = DefaultBuffer::new(4);
        let slot = black_box(sample).write_to(black_box(&mut buf));
        black_box(slot);
        black_box(&buf);
    }
    let elapsed = start.elapsed();
    println!(
        "  {:<6} {:>9} iters in {:>12?}  ({:>6.2} ns/iter)  [cold: fresh 4B buffer/iter]",
        name,
        COLD_N,
        elapsed,
        elapsed.as_nanos() as f64 / COLD_N as f64
    );
}

#[test]
fn fluffr_scalar_write_speed_hot() {
    println!("\n=== Fluffr Serialize::write_to — hot path (reused buffer) ===");
    bench_hot::<u8>("u8", 7u8);
    bench_hot::<u16>("u16", 1234u16);
    bench_hot::<u32>("u32", 123_456u32);
    bench_hot::<u64>("u64", 123_456_789u64);
    bench_hot::<u128>("u128", 123_456_789_012u128);
    bench_hot::<f32>("f32", 3.14159f32);
    bench_hot::<f64>("f64", 2.718281828f64);
}

#[test]
fn fluffr_scalar_write_speed_cold() {
    println!("\n=== Fluffr Serialize::write_to — cold path (fresh 4B buffer per call) ===");
    bench_cold::<u8>("u8", 7u8);
    bench_cold::<u32>("u32", 123_456u32);
    bench_cold::<u64>("u64", 123_456_789u64);
}