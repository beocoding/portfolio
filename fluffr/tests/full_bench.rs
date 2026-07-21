// Comprehensive fluffr benchmarks.
// Run: cargo test --release --test full_bench -- --nocapture --test-threads=1
// Each bench runs 5 timed repetitions and reports the MINIMUM ns/iter
// (standard microbenchmark practice to suppress scheduler noise).
// Output lines "BENCH <name> <ns>" are machine-parseable for A/B diffing.
use fluffr::*;
use std::hint::black_box;
use std::time::Instant;


// ── shared fixture types (self-contained — no external include) ──────────────

// ── Flat ──────────────────────────────────────────────────────────────────────

#[derive(Flat, Default, Clone, Copy)]
#[repr(C)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

// ── Nested table ──────────────────────────────────────────────────────────────

#[derive(Table, Default, Clone)]
pub struct Inner {
    pub id: u32,
    pub name: String,
}

// ── FlatUnion ─────────────────────────────────────────────────────────────────

#[derive(FlatUnion, Clone, Debug)]
#[repr(u8)]
pub enum Shape {
    None = 0,
    Num(u32) = 1,
    Node(Inner) = 2,
}
impl Default for Shape {
    fn default() -> Self {
        Shape::None
    }
}

// ── Root table exercising every field category ────────────────────────────────

#[derive(Table, Default, Clone)]
pub struct Root {
    pub a: u32,
    pub s: String,
    pub pt: Point,
    #[table]
    pub inner: Inner,
    #[union]
    pub shape: Shape,
    pub nums: Vec<u32>,
    pub names: Vec<String>,
    #[array(table)]
    pub items: Vec<Inner>,
}

// ── Merge-capable table (all list fields) ─────────────────────────────────────

#[derive(Table, Default, Clone)]
pub struct Reg {
    pub nums: Vec<u32>,
    pub names: Vec<String>,
}

fn sample_root() -> Root {
    Root {
        a: 42,
        s: "hello".into(),
        pt: Point { x: 7, y: 9 },
        inner: Inner { id: 1, name: "in".into() },
        shape: Shape::Num(99),
        nums: vec![1, 2, 3, 4],
        names: vec!["a".into(), "bb".into(), "ccc".into()],
        items: vec![
            Inner { id: 10, name: "x".into() },
            Inner { id: 11, name: "y".into() },
        ],
    }
}

// ── extra fixture types ───────────────────────────────────────────────────────

#[derive(Table, Default, Clone)]
pub struct Many {
    #[array(table)]
    pub items: Vec<Inner>,
}

#[derive(Table, Default, Clone)]
pub struct Mid {
    pub tag: u32,
    #[table]
    pub inner: Inner,
}

#[derive(Table, Default, Clone)]
pub struct Outer {
    pub tag: u32,
    #[table]
    pub mid: Mid,
}

// ── ROW-ONLY SECTION: delete from here to the matching marker if you use the
// Row-REMOVED macro variant ──────────────────────────────────────────────────
#[derive(Table, Row, Default, Clone)]
pub struct Person {
    #[key]
    pub id: u32,
    pub name: String,
    pub score: u32,
}

// ── harness ───────────────────────────────────────────────────────────────────

const REPS: usize = 5;

fn bench<F: FnMut() -> u64>(name: &str, iters: usize, mut f: F) {
    let mut sink = 0u64;
    for _ in 0..iters.min(2000) { sink = sink.wrapping_add(f()); } // warmup
    let mut best = u128::MAX;
    for _ in 0..REPS {
        let start = Instant::now();
        for _ in 0..iters { sink = sink.wrapping_add(f()); }
        best = best.min(start.elapsed().as_nanos());
    }
    black_box(sink);
    println!("BENCH {:<26} {:>10.1}", name, best as f64 / iters as f64);
}

// ── the suite ─────────────────────────────────────────────────────────────────

#[test]
fn full_bench() {
    println!("\n===== fluffr full benchmark suite (min of {REPS} reps, ns/iter) =====");

    // ---------- WRITE PATH ----------
    {
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_scalar_u32", 1_000_000, || {
            if b.head() < 64 { b.reset(); }
            Serialize::write_to(&black_box(123_456u32), &mut b) as u64
        });
    }
    {
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_scalar_u64", 1_000_000, || {
            if b.head() < 64 { b.reset(); }
            Serialize::write_to(&black_box(123_456_789u64), &mut b) as u64
        });
    }
    {
        let s = "hello world!";
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_string_short", 500_000, || {
            if b.head() < 128 { b.reset(); }
            Serialize::write_to(&black_box(s), &mut b) as u64
        });
    }
    {
        let s: String = "x".repeat(1024);
        let s = s.as_str();
        let mut b = DefaultBuffer::new(1 << 22);
        bench("w_string_1k", 200_000, || {
            if b.head() < 2048 { b.reset(); }
            Serialize::write_to(&black_box(s), &mut b) as u64
        });
    }
    {
        let v: Vec<u32> = (0..1000).collect();
        let hint = Serialize::size_hint(&v);
        let mut b = DefaultBuffer::new(1 << 22);
        bench("w_vec_u32_1k", 100_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&v, &mut b) as u64
        });
    }
    {
        let v: Vec<String> = (0..100).map(|i| format!("string_number_{i}")).collect();
        let hint = Serialize::size_hint(&v);
        let mut b = DefaultBuffer::new(1 << 22);
        bench("w_vec_string_100", 50_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&v, &mut b) as u64
        });
    }
    {
        let root = sample_root();
        let hint = Serialize::size_hint(&root);
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_root_mixed_hot", 200_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&root, &mut b) as u64
        });
    }
    {
        let root = sample_root();
        bench("w_root_mixed_cold", 50_000, || {
            let mut b = root.as_buffer();
            b.slot() as u64
        });
    }
    {
        let o = Outer { tag: 1, mid: Mid { tag: 2, inner: Inner { id: 3, name: "deep".into() } } };
        let hint = Serialize::size_hint(&o);
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_nested_3deep", 200_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&o, &mut b) as u64
        });
    }
    {
        let many = Many { items: (0..100).map(|i| Inner { id: i, name: format!("n{i}") }).collect() };
        let hint = Serialize::size_hint(&many);
        let mut b = DefaultBuffer::new(1 << 22);
        bench("w_table_list_100", 20_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&many, &mut b) as u64
        });
    }
    {
        let p = Person { id: 7, name: "solo".into(), score: 99 };
        let hint = Serialize::size_hint(&p) + 64;
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_row_as_registry", 200_000, || {
            if b.head() < hint { b.reset(); }
            b.ensure_capacity(hint);
            p.write_as_registry(&mut b) as u64
        });
    }

    // ---------- VIEW RE-SERIALIZE ----------
    {
        let root = sample_root();
        let mut src = root.as_buffer();
        let data: Vec<u8> = src.bytes().to_vec();
        let view = Root::view(&data, read_root(&data) as usize);
        let hint = Serialize::size_hint(&view);
        let mut b = DefaultBuffer::new(1 << 20);
        bench("w_view_reserialize", 100_000, || {
            if b.head() < hint + 64 { b.reset(); }
            Serialize::write_to(&view, &mut b) as u64
        });
    }

    // ---------- MERGE ----------
    {
        let r1 = Reg { nums: (0..64).collect(), names: (0..16).map(|i| format!("s{i}")).collect() };
        let r2 = Reg { nums: (64..128).collect(), names: (16..32).map(|i| format!("s{i}")).collect() };
        let mut b1 = r1.as_buffer();
        let d1: Vec<u8> = b1.bytes().to_vec();
        let slot1 = d1.len() - read_root(&d1) as usize;
        let mut b2 = r2.as_buffer();
        let d2: Vec<u8> = b2.bytes().to_vec();
        let v2 = Reg::view(&d2, read_root(&d2) as usize);
        let mut out = DefaultBuffer::new(1 << 16);
        bench("m_merge_2way", 100_000, || {
            v2.merge_into(&d1, &[slot1], &mut out);
            out.slot() as u64
        });
    }
    {
        // 8-way merge: one live view + 7 stored slots (same buffer, distinct tables)
        let regs: Vec<Reg> = (0..8).map(|k| Reg {
            nums: (k * 10..k * 10 + 10).collect(),
            names: (0..4).map(|i| format!("r{k}_{i}")).collect(),
        }).collect();
        // store 7 in one big buffer
        let mut store = DefaultBuffer::new(1 << 18);
        let slots: Vec<usize> = regs[..7].iter()
            .map(|r| Serialize::write_to(r, &mut store))
            .collect();
        let sdata: Vec<u8> = store.buffer()[store.head()..].to_vec();
        // slots are measured from end — still valid on the trimmed slice
        let mut b8 = regs[7].as_buffer();
        let d8: Vec<u8> = b8.bytes().to_vec();
        let v8 = Reg::view(&d8, read_root(&d8) as usize);
        let mut out = DefaultBuffer::new(1 << 18);
        bench("m_merge_8way", 30_000, || {
            v8.merge_into(&sdata, &slots, &mut out);
            out.slot() as u64
        });
    }

    // ---------- READ PATH ----------
    let root = sample_root();
    let mut src = root.as_buffer();
    let data: Vec<u8> = src.bytes().to_vec();
    let t_pos = read_root(&data) as usize;
    {
        bench("r_view_fields", 1_000_000, || {
            let v = Root::view(black_box(&data), black_box(t_pos));
            v.a() as u64 + v.s().len() as u64 + v.pt().x as u64 + v.inner().id() as u64
        });
    }
    {
        let big: Vec<u32> = (0..1000).collect();
        let holder = Reg { nums: big, names: vec![] };
        let mut hb = holder.as_buffer();
        let hd: Vec<u8> = hb.bytes().to_vec();
        let hv_pos = read_root(&hd) as usize;
        bench("r_list_u32_iter_1k", 100_000, || {
            let v = Reg::view(black_box(&hd), hv_pos);
            v.nums().map(|x| x as u64).sum::<u64>()
        });
        bench("r_list_u32_get_1k", 100_000, || {
            let v = Reg::view(black_box(&hd), hv_pos);
            let l = v.nums();
            let mut s = 0u64;
            for i in 0..l.len() { s += l.get(i) as u64; }
            s
        });
    }
    {
        let names: Vec<String> = (0..100).map(|i| format!("string_number_{i}")).collect();
        let holder = Reg { nums: vec![], names };
        let mut hb = holder.as_buffer();
        let hd: Vec<u8> = hb.bytes().to_vec();
        let hv_pos = read_root(&hd) as usize;
        bench("r_list_str_iter_100", 100_000, || {
            let v = Reg::view(black_box(&hd), hv_pos);
            v.names().map(|s| s.len() as u64).sum::<u64>()
        });
    }
    {
        let many = Many { items: (0..100).map(|i| Inner { id: i, name: format!("n{i}") }).collect() };
        let mut mb = many.as_buffer();
        let md: Vec<u8> = mb.bytes().to_vec();
        let mv_pos = read_root(&md) as usize;
        bench("r_table_list_iter_100", 50_000, || {
            let v = Many::view(black_box(&md), mv_pos);
            v.items().map(|e| e.id() as u64).sum::<u64>()
        });
    }
    {
        bench("r_union_read", 1_000_000, || {
            let v = Root::view(black_box(&data), t_pos);
            match v.shape() { ShapeView::Num(n) => n as u64, _ => 0 }
        });
    }

    // ---------- ROW / REGISTRY READ ----------
    {
        let n = 1000u32;
        let reg = PersonRegistry {
            id:    (0..n).collect(),
            name:  (0..n).map(|i| format!("p{i}")).collect(),
            score: (0..n).map(|i| i * 2).collect(),
        };
        let mut rb = reg.as_buffer();
        let rd: Vec<u8> = rb.bytes().to_vec();
        let rv_pos = read_root(&rd) as usize;
        bench("r_registry_rows_100", 50_000, || {
            let v = PersonRegistry::view(black_box(&rd), rv_pos);
            let mut s = 0u64;
            for i in 0..100 { let r = v.get_row(i); s += r.id as u64 + r.name.len() as u64; }
            s
        });
        bench("q_by_key_hit_last", 50_000, || {
            let v = PersonRegistry::view(black_box(&rd), rv_pos);
            v.query_by_key(black_box(n - 1)).unwrap_or(0) as u64
        });
        bench("q_builder_score", 20_000, || {
            let v = PersonRegistry::view(black_box(&rd), rv_pos);
            let m = v.query(PersonQuery::default().score(black_box(500 * 2)));
            m.iter().count() as u64
        });
    }

    // ── END ROW-ONLY SECTION (also delete the `w_row_as_registry` and
    // `r_registry_rows_100`/`q_*` benches above if Row is removed) ────────────

    // ---------- VERIFY ----------
    {
        bench("v_verify_root_mixed", 200_000, || {
            verify_root::<Root>(black_box(&data)).is_ok() as u64
        });
    }
}