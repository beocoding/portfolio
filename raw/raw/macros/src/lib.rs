// macros/src/lib.rs

#[derive(Debug, Clone, PartialEq)]
enum TypeClass {
    Bool,
    Int8, UInt8,
    Int16, UInt16,
    Int32, UInt32,
    Int64, UInt64,
    Float32, Float64,
    String,
    Vector(Box<TypeClass>),
    Struct,   // fixed-size scalar group, stored inline
    Table,    // vtable-indexed, behind an offset
}

trait Classify {
    const CLASS_KIND: Kind; // used to distinguish struct vs table statically
    fn type_class() -> TypeClass;
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind { Scalar, String, Vector, Struct, Table }

macro_rules! scalar {
    ($t:ty, $c:expr) => {
        impl Classify for $t {
            const CLASS_KIND: Kind = Kind::Scalar;
            fn type_class() -> TypeClass { $c }
        }
    };
}

scalar!(bool, TypeClass::Bool);
scalar!(i8,  TypeClass::Int8);   scalar!(u8,  TypeClass::UInt8);
scalar!(i16, TypeClass::Int16);  scalar!(u16, TypeClass::UInt16);
scalar!(i32, TypeClass::Int32);  scalar!(u32, TypeClass::UInt32);
scalar!(i64, TypeClass::Int64);  scalar!(u64, TypeClass::UInt64);
scalar!(f32, TypeClass::Float32);
scalar!(f64, TypeClass::Float64);

impl Classify for String {
    const CLASS_KIND: Kind = Kind::String;
    fn type_class() -> TypeClass { TypeClass::String }
}

impl<T: Classify> Classify for [T] {
    const CLASS_KIND: Kind = Kind::Vector;
    fn type_class() -> TypeClass { TypeClass::Vector(Box::new(T::type_class())) }
}