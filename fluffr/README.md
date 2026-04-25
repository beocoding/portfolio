## Table of Contents

- [Overview](#overview)
  - [Why Fluffr?](#why-fluffr)
    - [Automatic schema generation](#automatic-schema-generation)
    - [Performance and efficiency](#performance-and-efficiency)
    - [Forwards and backwards compatibility](#forwards-and-backwards-compatibility)
- [Quickstart](#quickstart)
  - [Supported Primitive Types](#supported-primitive-types)
  - [Derives](#derives)
    - [#[derive(Table)]](#derivetable)
    - [#[derive(Flat)]](#deriveflat)
    - [#[derive(FlatUnion)]](#deriveflatunion)
    - [#[derive(Row)]](#deriverow)
  - [Buffer Trait](#buffer-trait)
    - [DefaultBuffer](#defaultbuffer)
    - [Methods](#methods)
  - [Serialize Trait](#serialize-trait)
    - [size_hint](#size_hint)
    - [write_to](#write_to)
    - [write_to_unchecked](#write_to_unchecked)
  - [ReadAt Trait](#readat-trait)
    - [read_at](#read_at)
    - [ListView](#listview)
- [Benchmark Results](#benchmark-results)
  - [Product Type Breakdown](#product-type-breakdown)
  - [AoS vs SoA Memory Layout](#aos-vs-soa-memory-layout)
  - [Parameters Tested](#parameters-tested)
  - [Single Product](#single-product)
  - [Registry Encode](#registry-encode)
  - [Registry Decode](#registry-decode)
  - [Registry Network Latency](#registry-network-latency)

---

# Overview
Fluffr is a bit-level serialization library heavily inspired by [Flatbuffers](https://flatbuffers.dev/).  It allows the user to encode their data structs into a binary format that can be read back without deserialization. Field access happens directly against the raw bytes through zero-copy view types derived at compile time via proc-macros.

## Why Fluffr?

### **Automatic schema generation**

Libraries like FlatBuffers and Protocol Buffers require a predefined schema and typically involve a code generation step, producing types used to read/write structured data. JSON avoids schema and codegen, but requires runtime parsing and may involve additional structs for typed access.

Fluffr takes a different approach. Your existing Rust structs become your schema. A few derive macros and field attributes are all that is needed — no separate schema language, no code generation step, and no parallel type hierarchy to keep in sync.

For more advanced use cases, Fluffr also offers an optional columnar storage format via the #[derive(Row)] macro. Deriving Row on a struct automatically generates a companion registry type that stores each field as its own packed column — a struct-of-arrays layout rather than the default array-of-structs. This makes it well suited to query-heavy workloads where you need to scan a single field across many records without touching the rest. Libraries like Flatbuffers and Protocol Buffers have no equivalent concept; achieving the same layout with them would require manually maintaining a separate schema and type.



### **Performance and efficiency**
Fluffr views resolve field positions through vtable offsets, so only the fields you actually access are ever touched — there is no upfront parse step. The only memory required to read your data is the buffer itself; views are lightweight structs holding a reference to the raw bytes and allocate nothing on the heap. Fluffr has minimal dependencies and a small code footprint.

### **Forwards and backwards compatibility**
Because fields are located by vtable offset rather than position, schemas can grow over time. Old readers silently skip fields they don't know about; new readers fall back to defaults for fields that aren't present in older data.

---
# QuickStart
***
## Supported Primitive Types  
***
`Serialize` and `ReadAt` are implemented out of the box for the following types:

-**Integer and float scalars** — stored inline at their natural alignment, in little-endian byte order:
`u8`, `u16`, `u32`, `u64`, `u128`, `i8`, `i16`, `i32`, `i64`, `i128`, `f32`, `f64`

-**Strings** — `String` and `&str` are stored as a 4-byte length prefix followed by the UTF-8 bytes at a 4-byte-aligned position. A forward offset to the string is written into the parent table.

-**Arrays** — `Vec<T>` and `&[T]` are supported for any serializable element type. Inline element types (scalars, `Flat` structs) are packed as a contiguous block preceded by a length prefix. Offset element types (strings, tables) are written individually and referenced through a forward-offset table.

-**`Flat` structs and enums** — any type deriving `Flat` is treated as an inline scalar and stored as its raw little-endian bytes.

-**`Table` types** — any type deriving `Table` can be serialized as a nested field, referenced via a forward offset from the parent table.

-**`FlatUnion` enums** — stored as a 5-byte slot: a 4-byte forward offset to the payload and a 1-byte tag identifying the variant.

<br>

***

## Derives

***
### #[derive(Table)]
use #[derive(Table)] on any struct to make it serializable. Tables can have the following attributes per field:

- scalar — plain numeric types and #[repr(C)] POD structs
- string — String or &str
- table — a nested table struct
- union — a FlatUnion enum
- file — a FileBlob<T> typed binary blob
- array(scalar) / array(string) / array(table) / array(union) / array(file) — a Vec<T> of any of the above

```#[derive(Table)]``` is capable of automatically detecting any fields that are not Tables or Unions. Fields that are Table or Union types must be annotated using ```#[table]``` or ```#[union]```, respectively. Attributing a ```Vec<Table>``` as ```#[table]``` or ```Vec<Union>``` field as ```#[union]``` is also automatically detected as ```#[array(table)]``` or ```#[array(union)]```, respectively.
<br> <br>
#### Table Example with field attributes:
```rust
use fluffr::prelude::*;

#[derive(Table, Clone, Default)]
pub struct BrandData {
    #[string]  pub name: String,
}

#[derive(Table, Default)]
pub struct Product {
    #[table]   pub brand:      BrandData,
    #[string]  pub sku:        String,
    #[string]  pub label:      String,
    #[scalar]  pub price:      f32,
    #[scalar]  pub weight:     f32,
}
```

<br>

#### Example with auto-field attributes:

```rust
use fluffr::prelude::*;

#[derive(Table, Clone, Default)]
pub struct BrandData {
    pub name: String,
}

#[derive(Table, Default)]
pub struct Product {
    #[table]	
    pub brand:      BrandData,
    pub sku:        String,
    pub label:      String,
    pub price:      f32,
    pub weight:     f32,
}
```

<br>

***

### Methods

`#[derive(Table)]` generates the following methods on the struct and its companion view type:

**On the owned struct:**

- `write_to<B: Buffer>(&self, buf: &mut B) -> usize` — serializes the struct into the buffer and returns its slot.
- `view_at_slot<'a>(buf: &'a [u8], slot: usize) -> TableView<'a>` — constructs a view directly from a known slot, without needing `read_root`.
- `as_buffer(&self) -> DefaultBuffer` — serializes the struct into a fresh `DefaultBuffer` and returns it finished.

**On the generated `TableView<'a>` type:**

- `new(buf: &'a [u8], table_pos: usize) -> Self` — constructs a view at an absolute byte position.
- `from_slot(buf: &'a [u8], slot: usize) -> Self` — constructs a view from a slot.
- `block_end(&self) -> usize` — returns the byte position of the end of this table's data block, used internally during merging.
- One accessor method per field, named after the field, returning the field's value read directly from the buffer with no allocation.

**On the `Table` trait (via `ReadAt`):**

- `view<'a>(buf: &'a [u8], table_idx: usize) -> TableView<'a>` — the primary entry point for reading. Pass the result of `read_root` as `table_idx`.

***

### #[derive(Flat)]

`#[derive(Flat)]` marks a type as an inline scalar value — stored directly in the buffer with no indirection. It can be applied to both structs and enums.

<br>

#### NewTypes

A `Flat` struct must be a Plain Old Data type: all fields must themselves be `Flat`, and the struct must be `#[repr(C)]`. It is stored and read back as raw bytes with no vtable overhead.

```rust
#[repr(C)]
#[derive(Flat, Copy, Clone, Default)]
pub struct Dimensions {
    pub width:  f32,
    pub height: f32,
    pub depth:  f32,
}
```

A `Flat` struct can then be used as a `#[scalar]` field inside a `Table`.

<br>

#### Enums

A `Flat` enum must have a primitive `#[repr(...)]` attribute. It is stored as its underlying integer type and transmuted back on read.

```rust
#[derive(Flat, Clone, Copy, Default)]
#[repr(u8)]
pub enum ProductCategory {
    #[default]
    None,
    Electronics,
    Apparel,
    Home,
    Grocery,
}
```

Like `Flat` structs, `Flat` enums are used as `#[scalar]` fields inside a `Table`.

<br>

***

### #[derive(Union)]

`#[derive(FlatUnion)]` marks an enum as a discriminated union — a field that can hold one of several typed variants. Unions must be `#[repr(u8)]` and must have a unit variant with discriminant `0` to serve as the absent/none sentinel.

Each non-none variant holds a single payload, which can be a `String`, a `Flat` scalar struct, or a nested `Table`.

```rust
#[derive(FlatUnion, Clone, Default)]
#[repr(u8)]
pub enum ProductLink {
    #[default]
    None,
    BySKU(String),
    ByDims(Dimensions),
    ByBrand(BrandData),
}
```

A `FlatUnion` is used as a `#[union]` field inside a `Table`:

```rust
#[derive(Table, Default)]
pub struct Product {
    #[string] pub sku:   String,
    #[scalar] pub price: f32,
    #[union]  pub link:  ProductLink,
}
```

On read, the view exposes the union as a generated `ProductLinkView` enum that can be matched on directly:

```rust
match view.link() {
    ProductLinkView::None          => { /* no link */ }
    ProductLinkView::BySKU(sku)    => println!("linked by SKU: {}", sku),
    ProductLinkView::ByDims(dims)  => println!("width: {}", dims.width),
    ProductLinkView::ByBrand(b)    => println!("brand: {}", b.name()),
}
```


<br>

---

### `#[derive(Row)]`

`#[derive(Row)]` can be added to any struct that also derives `Table`. It generates a companion registry type that stores each field as its own packed column — a struct-of-arrays layout rather than the default array-of-structs. This makes it well suited to query-heavy workloads where you need to scan a single field across many records without touching the rest.

```rust
#[derive(Row, Table, Clone, Default)]
pub struct Product {
    #[table]  pub brand:      BrandData,
    pub sku:        String,
    pub label:      String,
    pub price:      f32,
    pub weight:     f32,
}
```

This generates a `ProductRegistry` struct where each field becomes a `Vec<T>` column, and a `ProductRegistryView` for zero-copy reading. The registry itself derives `Table`, so it serializes and reads back through the same buffer machinery as any other table.

<br>

#### `RowRef`

Alongside the registry, `#[derive(Row)]` generates a `ProductRowRef<'a>` type — a lightweight struct holding the decoded read output for every field of a single row. It is returned by `RegistryView::get_row(i)` and can be compared directly against an owned `Product` or a `ProductView`.

```rust
let root     = read_root(&bytes) as usize;
let registry = ProductRegistry::view(&bytes, root);
let row      = registry.get_row(0);  // ProductRowRef<'_>

println!("{}", row.sku);    // &str, zero-copy
println!("{}", row.price);  // f32
```

<br>

***

## Buffer trait

***
### DefaultBuffer

`DefaultBuffer` is the standard buffer used to serialize tables into bytes. It grows backward from the high end of its allocation toward the low end, and all positions are expressed as slots — distances from the end of the buffer — so they remain stable if the buffer needs to grow.

To serialize a table, write it into a `DefaultBuffer` and call `finish` to prepend the root offset and get the final byte slice:

```rust
let mut buf = DefaultBuffer::default();
let slot    = my_table.write_to(&mut buf);
let bytes   = buf.finish(slot).to_vec();
```

To read back, use `read_root` to recover the root position and pass it to the table's `view` method:

```rust
let root = read_root(&bytes) as usize;
let view = MyTable::view(&bytes, root);
```

<br>

#### Reuse across serializations

`DefaultBuffer` can be reused across multiple serializations without reallocating. Calling `reset()` resets the write frontier without freeing the backing allocation. `merge_into` calls `reset` automatically on entry, so a single buffer can be held across many merge calls:

```rust
let mut buf = DefaultBuffer::default();

for record in records {
    record.merge_into(&mut buf, &other);
}
```

<br>

#### Vtable deduplication

When serializing a list of tables that share the same schema, `DefaultBuffer` automatically deduplicates their vtables — only one physical copy is written to the buffer regardless of how many elements are present. This is handled internally and requires no configuration.

<br>

*** 

### Methods

**Required** — must be implemented by any custom `Buffer`:

- `new(initial_capacity: usize) -> Self` — allocates a new buffer with at least the given capacity.
- `head(&self) -> usize` — returns the current write frontier: the index of the first valid byte.
- `head_mut(&mut self) -> &mut usize` — returns a mutable reference to the write frontier, used to advance it with `*buf.head_mut() -= n`.
- `buffer(&self) -> &[u8]` — returns the full backing byte slice, including the unwritten low region.
- `buffer_mut(&mut self) -> &mut [u8]` — returns a mutable view of the full backing byte slice.
- `grow(&mut self, new_cap: usize)` — grows the backing allocation to exactly `new_cap` bytes, shifting all existing written data toward the high end and updating `head` accordingly.
- `share_vtable(&mut self, vtable: &[u8], table_slot: usize)` — writes a vtable if not already present and patches the jump field of the table object at `table_slot`.
- `clear_vtables(&mut self)` — clears the vtable deduplication cache.
- `load<T: Table>(bytes: &[u8]) -> Self` — constructs a buffer from an existing finished byte slice.

<br>

**Provided** — implemented automatically in terms of the required primitives:

- `len(&self) -> usize` — total capacity of the backing allocation in bytes.
- `slot(&self) -> usize` — distance from the end of the buffer to the current write frontier. This is the stable address of the most recently written object and remains valid across `grow` calls.
- `ensure_capacity(&mut self, additional_size: usize)` — ensures `additional_size` bytes are available below `head`, calling `grow` if necessary.
- `align(&mut self, alignment: usize)` — aligns `head` downward to the given alignment, which must be a power of two.
- `reset(&mut self)` — resets `head` to `len` and clears the vtable cache without freeing the backing allocation. Called automatically by `merge_into`.
- `bytes(&self) -> &[u8]` — returns the finished, readable byte slice starting at `head`. Only valid after `finish` has been called.
- `finish(&mut self, slot: usize) -> &[u8]` — writes the 4-byte root prefix encoding the offset to the root table object, and returns the finished byte slice.

<br>

***
## Serialize trait
***
### `size_hint(&self) -> usize`

`size_hint` returns an upper bound on the number of bytes a value will consume when written, including alignment padding. It is used by `write_to` and parent tables to call `ensure_capacity` once before entering the unchecked write path.

Per-type bounds are as follows: scalars return `size_of::<T>() + align_of::<T>() - 1` to account for worst-case alignment padding; strings return `len + 11` (4 bytes for the length prefix, 4 bytes for alignment, 3 bytes worst-case padding); arrays accumulate the hints of their elements; and `Table` types sum the hints of all their fields plus vtable overhead.

`size_hint` is conservative by design — it may overestimate, but never underestimates. This means `ensure_capacity(value.size_hint())` is always a safe precondition for calling `write_to_unchecked`.

***
### write_to<S: Serialize, B: Buffer>(&S, buf: &mut B) -> usize


`write_to` is available on any type that implements `Serialize` — including primitives, strings, `Flat` structs and enums, `FlatUnion` enums, arrays, and `Table` structs. It writes the value into the provided buffer and returns a slot — a stable offset from the end of the buffer — that identifies where the value was written.

```rust
let mut buf = DefaultBuffer::default();
let slot    = my_table.write_to(&mut buf);
let bytes   = buf.finish(slot).to_vec();
```

When serializing a root table, the returned slot is passed to `finish` to write the root prefix. When writing a nested value, the slot is used by the parent to write a forward offset to it.

<br>

#### Absent field elision

Fields that are in their default or zero state are not written to the buffer at all. Their vtable entry is left as `0`, which signals to the reader that the field is absent. Sparse tables are therefore stored efficiently with no wasted bytes for unpopulated fields.

<br>

#### Field ordering

For simple scalar values or strings, those get written directly into the next slot of the Buffer.  

For structs that are derived as `Flat`, the fields are serialized in the order they are declared in. For structs that are derived as 'Table', it becomes a bit more nuanced. Vtable slots are assigned in declaration order, so field positions are stable and predictable. Internally, serialization uses a two-pass approach: indirect fields (strings, tables, unions, arrays) are written before the table object that references them, as required by the backward-growing buffer layout. This is an implementation detail and has no effect on how fields are declared or accessed.

For FlatUnion types, the write order of the payload follows the same rules as the inner type — inline for Flat scalars and structs, two-pass for nested Table payloads, and offset-based for String payloads.


<br>

***

### write_to_unchecked<S: Serialize, B: Buffer>(&S, buf: &mut B) -> usize


`write_to_unchecked` is the same as `write_to` but skips the upfront capacity check. It is used internally by parent tables during serialization: the parent calls `ensure_capacity` once for all of its fields combined and then writes each one through the unchecked path, avoiding a redundant check per field.

In most cases you should call `write_to` directly. `write_to_unchecked` is only appropriate when you have already guaranteed sufficient space in the buffer:

```rust
buf.ensure_capacity(my_value.size_hint());
my_value.write_to_unchecked(&mut buf);
```

Calling `write_to_unchecked` without a prior capacity guarantee will panic on the internal slice indexing in debug builds, and produce silent memory corruption in release builds with `debug_assert` disabled.

<br>

***

## ReadAt<'a> Trait
***

`ReadAt<'a>` is the read-side counterpart to `Serialize`. It decodes a value of a given type from a byte buffer at an absolute byte position, with the lifetime `'a` tying any returned references directly to the input slice. No allocation occurs on the read path.

***
### read_at(buf: &'a [u8], offset: usize) -> Self::ReadOutput

Decodes a value from `buf` at the given absolute byte position. The type returned is `Self::ReadOutput`, which varies by type:

| Type | `ReadOutput` |
|---|---|
| Scalars (`u8`, `f32`, etc.) | The scalar value itself |
| `bool` | `bool` |
| `String` / `&str` | `&'a str` — a zero-copy borrow into `buf` |
| `Vec<T>` | `ListView<'a, T>` |
| `Flat` struct or enum | `Self` — read back as raw bytes and reinterpreted inline |
| `#[derive(Table)]` struct | The generated `{TypeName}View<'a>` (e.g. `ProductView<'a>`) — a zero-copy handle into the buffer |
| `#[derive(FlatUnion)]` enum | The generated `{TypeName}View<'a>` enum (e.g. `ProductLinkView<'a>`) — a matched variant holding the decoded payload |

The `MODE` constant on each implementation tells the parent table whether this field's bytes are stored inline at the field position (`Inline`) or whether the field position holds a forward offset to the actual data (`Offset`). `Flat` types are always `Inline` — their bytes sit directly at the field position and are reinterpreted in place with no indirection. `Table` and `FlatUnion` views are always `Offset` — the field position holds a forward offset to the actual data, and the view borrows from the buffer at that location. This distinction is resolved at compile time and requires no branching at runtime.

***

### ListView<'a, T>

Array fields return a `ListView` rather than a `Vec`. It is a zero-copy, double-ended, random-access view over an array stored in the buffer, and implements both `Iterator` and `DoubleEndedIterator`. No elements are decoded until accessed.

```rust
let tags = view.tags();     // ListView<'a, &str>
let first = tags.get(0);    // Option<&str>
for tag in tags { ... }     // iterates without allocating
```

`ListView` also supports an optional skip list via `with_skip`, which causes the iterator to omit specific indices. This is used internally by the query system to iterate only over rows matching a predicate.

<br>

---
# Benchmark Results
---

Benchmarks compare **JSON**, **Protobuf** (`proto`), **FlatBuffers** (`flatbuf`), and **Flatr** (`Flatr`) across encode, decode, and network latency. All times are Criterion median values. Network latency = decode + full field traversal of every field in a single timed block.

***
## Product Type Breakdown

A single `Product` record has 11 fields spanning five distinct storage categories.

| Field       | Type                                                       | Notes                        |
| ----------- | ---------------------------------------------------------- | ---------------------------- |
| sku         | String                                                     | Primary key                  |
| label       | String                                                     |                              |
| slug        | String                                                     |                              |
| description | String                                                     |                              |
| brand       | Brand { name: String }                                     | Nested struct                |
| tags        | Vec<String>                                                | Variable-length string array |
| category    | enum (5 variants)                                          | Scalar discriminant          |
| price       | f32                                                        |                              |
| weight      | f32                                                        |                              |
| dimensions  | { width, height, depth: f32 }                              | Inline #[repr(C)] struct     |
| link        | enum { None, BySku(String), ByDims(Dims), ByBrand(Brand) } | Tagged union                 |


## AoS vs SoA Memory Layout

For JSON, Proto, and FlatBuffers, a registry of N products is **Array of Structs**:

```
[ { sku, label, slug, desc, brand, tags, category, price, weight, dims, link },   // product 0
  { sku, label, slug, desc, brand, tags, category, price, weight, dims, link },   // product 1
  ... ]
```

Reading `price` for all N products requires striding through the entire buffer, touching all 11 fields per record.

Flatr's `#[derive(Row)]` generates a **Struct of Arrays** registry:

```
{
  sku:        [ "SKU-000000", "SKU-000001", ... ],   // contiguous string column
  label:      [ "Product Label 0", ... ],
  slug:       [ ... ],
  description:[ ... ],
  brand:      [ Brand0, Brand1, ... ],               // nested table column
  tags:       [ Tags0, Tags1, ... ],                 // nested array column
  category:   [ 0u8, 1u8, 2u8, ... ],               // packed enum column
  price:      [ 9.99, 10.99, 11.99, ... ],           // packed f32 slice
  weight:     [ 0.50,  0.60,  0.70, ... ],           // packed f32 slice
  dimensions: [ Dims0, Dims1, ... ],                 // packed #[repr(C)] structs
  link:       [ None, BySku(...), ByDims(...), ... ] // union column
}
```

Reading `price` for all N products is a single linear scan over a `[f32; N]` — one cache line burst with no pointer chasing.

***
## Parameters Tested

### Encode
Encode is the time it takes to serialize data.

### Decode
Decode is the time it takes from receiving a buffer and preparing it for reading.

### Network Latency
Network Latency measures the time to decode and then read each field.

***

## Single Product

| Format  | Encode (ns) | Decode (ns) | Network Latency (ns) |
| ------- | ----------- | ----------- | -------------------- |
| JSON    | 356.8       | 597.1       | 593.2                |
| Proto   | **90.5**    | 240.7       | 236.7                |
| FlatBuf | 361.4       | 337.2       | 359.8                |
| FlatR   | 167.7       | **4.97**    | **6.41**             |

Proto is the fastest encoder at 90.5 ns — 4× faster than JSON and FlatBuffers which are nearly identical (~357–361 ns). 

FlatR decode is 120× faster than Proto at 4.97 ns — the zero-copy view returns immediately. FlatBuf decode at 337 ns is slower than Proto despite also being lazy, because flatbuffers::root() runs a buffer size and alignment check that Proto's field-by-field parse happens to edge out at this payload size.
***

## Registry Encode

| Format  | 100 products    | 500 products      | 1,000 products    | 10,000 products   |
| ------- | -----           | -----             | -----             | ------            |
| JSON    | 29.6 us         | 152.8 us          | 306.3 us          | 3,069 us          |
| Proto   | 8.85 us         | 43.1 us           | 86.9 us           | 879 us            |
| FlatBuf | 31.5 us         | 159.4 us          | 325.2 us          | 3,238 us          |
| FlatR   | **6.82 us**     | **34.8 us**       | **69.2 us**       | **684 us**        |

Proto leads at every size, but FlatR closes the gap aggressively at scale — at 10,000 items FlatR is 22% faster than Proto (684 µs vs 879 µs), because columnar layout amortizes string encoding across contiguous memory rather than per-object vtable writes. FlatBuffers and JSON are consistently 3–4× slower than Proto at every size.
***

## Registry Decode

| Format  | 100 products         | 500 products           | 1,000 products         | 10,000 products        |
| ------- | -------              | --------               | --------               | -------                |
| JSON    | 68.3 µs              | 348.0 µs               | 690.1 µs               | 6.95 ms                |
| Proto   | 35.2 µs              | 178.1 µs               | 356.5 µs               | 3.57 ms                |
| FlatBuf | 35.1 µs              | 176.9 µs               | 355.4 µs               | 3.55 ms                |
| FlatR   | **4.89 ns**          | **4.97 ns**            | **4.85 ns**            | **4.90 ns**            |

FlatR decode is constant ~4.9 ns at every registry size — it doesn't scale with n at all because it's a single root pointer cast into columnar arrays. Proto and FlatBuffers are nearly identical (both eager field parsing under the hood when you force access), each tracking about 2× faster than JSON.
***

## Registry Network Latency

| Format  | 100 products     | 500 products      | 1,000 products    | 10,000 products  |
| ------- | -------          | --------          | --------          | -------          |
| JSON    | 67.7 µs          | 347.8 µs          | 701.6 µs          | 7.11 ms          |
| Proto   | 36.2 µs          | 177.2 µs          | 350.9 µs          | 3.56 ms          |
| FlatBuf | 37.3 µs          | 190.9 µs          | 374.8 µs          | 3.77 ms          |
| FlatR   | 618 ns           | 3.10 µs           | 6.28 µs           | 66.7 µs          |

This is the most realistic benchmark — it forces a full decode plus traversal of every field on every item, matching a real receive-and-read workload. FlatR scales linearly but remains ~53× faster than Proto at 10,000 items (66.7 µs vs 3.56 ms), because column reads stride through contiguous memory while Proto and FlatBuffers must chase per-object pointers across the heap. FlatBuffers is consistently ~5–8% slower than Proto despite identical lazy semantics, reflecting vtable indirection overhead per field access.

***
