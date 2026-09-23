//! RFC 8949 core deterministic CBOR: the value model, its encoder, and a serde serializer
//! into it.

use super::EncodeError;
use serde::ser::{self, Serialize};

/// A CBOR data item of the kinds the kernel's records produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// An unsigned integer (major type 0).
    Uint(u64),
    /// The negative integer `-1 - n` (major type 1).
    Nint(u64),
    /// A byte string (major type 2).
    Bytes(Vec<u8>),
    /// A text string (major type 3).
    Text(String),
    /// An array (major type 4).
    Array(Vec<Value>),
    /// A map (major type 5), in the order its entries were produced; the encoder sorts it.
    Map(Vec<(Value, Value)>),
    /// `false` or `true` (major type 7).
    Bool(bool),
    /// `null` (major type 7).
    Null,
}

impl Value {
    /// The core deterministic encoding of the value.
    ///
    /// # Errors
    ///
    /// [`EncodeError::DuplicateKey`] if a map holds two keys with the same encoding.
    pub(super) fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Uint(n) => head(out, 0, *n),
            Self::Nint(n) => head(out, 1, *n),
            Self::Bytes(b) => {
                head(out, 2, len(b.len()));
                out.extend_from_slice(b);
            }
            Self::Text(s) => {
                head(out, 3, len(s.len()));
                out.extend_from_slice(s.as_bytes());
            }
            Self::Array(items) => {
                head(out, 4, len(items.len()));
                for item in items {
                    item.encode_into(out)?;
                }
            }
            Self::Map(entries) => {
                let mut encoded = entries
                    .iter()
                    .map(|(k, v)| Ok((k.encode()?, v)))
                    .collect::<Result<Vec<_>, EncodeError>>()?;
                encoded.sort_by(|a, b| a.0.cmp(&b.0));
                if encoded.windows(2).any(|w| w[0].0 == w[1].0) {
                    return Err(EncodeError::DuplicateKey);
                }
                head(out, 5, len(encoded.len()));
                for (key, value) in encoded {
                    out.extend_from_slice(&key);
                    value.encode_into(out)?;
                }
            }
            Self::Bool(false) => out.push(0xf4),
            Self::Bool(true) => out.push(0xf5),
            Self::Null => out.push(0xf6),
        }
        Ok(())
    }
}

/// A length as a CBOR argument; `usize` is at most 64 bits on every supported target.
fn len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Appends the shortest head of major type `major` with argument `n`.
fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let major = major << 5;
    if let Ok(small) = u8::try_from(n)
        && small < 24
    {
        out.push(major | small);
    } else if let Ok(b) = u8::try_from(n) {
        out.extend_from_slice(&[major | 24, b]);
    } else if let Ok(b) = u16::try_from(n) {
        out.push(major | 25);
        out.extend_from_slice(&b.to_be_bytes());
    } else if let Ok(b) = u32::try_from(n) {
        out.push(major | 26);
        out.extend_from_slice(&b.to_be_bytes());
    } else {
        out.push(major | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

/// The [`Value`] serde produces for `value`.
///
/// # Errors
///
/// [`EncodeError::Float`] for a floating-point number, [`EncodeError::IntegerRange`] for a
/// 128-bit integer outside the CBOR integer range, and [`EncodeError::Custom`] for an error a
/// `Serialize` implementation raises.
pub(super) fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, EncodeError> {
    value.serialize(ValueSerializer)
}

/// Serializes into a [`Value`].
struct ValueSerializer;

/// The value of a signed integer.
fn int(v: i64) -> Value {
    match u64::try_from(v) {
        Ok(n) => Value::Uint(n),
        Err(_) => Value::Nint(v.unsigned_abs() - 1),
    }
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = EncodeError;
    type SerializeSeq = SeqBuilder;
    type SerializeTuple = SeqBuilder;
    type SerializeTupleStruct = SeqBuilder;
    type SerializeTupleVariant = VariantBuilder<SeqBuilder>;
    type SerializeMap = MapBuilder;
    type SerializeStruct = MapBuilder;
    type SerializeStructVariant = VariantBuilder<MapBuilder>;

    fn serialize_bool(self, v: bool) -> Result<Value, EncodeError> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, EncodeError> {
        Ok(int(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, EncodeError> {
        Ok(int(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, EncodeError> {
        Ok(int(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, EncodeError> {
        Ok(int(v))
    }
    fn serialize_i128(self, v: i128) -> Result<Value, EncodeError> {
        match u64::try_from(v) {
            Ok(n) => Ok(Value::Uint(n)),
            Err(_) => u64::try_from(-1 - v)
                .map(Value::Nint)
                .map_err(|_| EncodeError::IntegerRange),
        }
    }
    fn serialize_u8(self, v: u8) -> Result<Value, EncodeError> {
        Ok(Value::Uint(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, EncodeError> {
        Ok(Value::Uint(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, EncodeError> {
        Ok(Value::Uint(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, EncodeError> {
        Ok(Value::Uint(v))
    }
    fn serialize_u128(self, v: u128) -> Result<Value, EncodeError> {
        u64::try_from(v)
            .map(Value::Uint)
            .map_err(|_| EncodeError::IntegerRange)
    }
    fn serialize_f32(self, _: f32) -> Result<Value, EncodeError> {
        Err(EncodeError::Float)
    }
    fn serialize_f64(self, _: f64) -> Result<Value, EncodeError> {
        Err(EncodeError::Float)
    }
    fn serialize_char(self, v: char) -> Result<Value, EncodeError> {
        Ok(Value::Text(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Value, EncodeError> {
        Ok(Value::Text(v.to_owned()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, EncodeError> {
        Ok(Value::Bytes(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Value, EncodeError> {
        Ok(Value::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, EncodeError> {
        to_value(value)
    }
    fn serialize_unit(self) -> Result<Value, EncodeError> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, EncodeError> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value, EncodeError> {
        Ok(Value::Text(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Value, EncodeError> {
        to_value(value)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, EncodeError> {
        Ok(Value::Map(vec![(
            Value::Text(variant.to_owned()),
            to_value(value)?,
        )]))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqBuilder, EncodeError> {
        Ok(SeqBuilder(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqBuilder, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantBuilder<SeqBuilder>, EncodeError> {
        Ok(VariantBuilder {
            variant,
            inner: SeqBuilder(Vec::with_capacity(len)),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapBuilder, EncodeError> {
        Ok(MapBuilder {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<MapBuilder, EncodeError> {
        self.serialize_map(Some(len))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantBuilder<MapBuilder>, EncodeError> {
        Ok(VariantBuilder {
            variant,
            inner: self.serialize_map(Some(len))?,
        })
    }
}

/// Collects the elements of an array.
pub(super) struct SeqBuilder(Vec<Value>);

impl SeqBuilder {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        self.0.push(to_value(value)?);
        Ok(())
    }
}

impl ser::SerializeSeq for SeqBuilder {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Value::Array(self.0))
    }
}

impl ser::SerializeTuple for SeqBuilder {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Value::Array(self.0))
    }
}

impl ser::SerializeTupleStruct for SeqBuilder {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Value::Array(self.0))
    }
}

/// Collects the entries of a map or the fields of a struct.
pub(super) struct MapBuilder {
    entries: Vec<(Value, Value)>,
    key: Option<Value>,
}

impl MapBuilder {
    fn field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        self.entries
            .push((Value::Text(name.to_owned()), to_value(value)?));
        Ok(())
    }
}

impl ser::SerializeMap for MapBuilder {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), EncodeError> {
        self.key = Some(to_value(key)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| EncodeError::Custom("a map value without its key".to_owned()))?;
        self.entries.push((key, to_value(value)?));
        Ok(())
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Value::Map(self.entries))
    }
}

impl ser::SerializeStruct for MapBuilder {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        self.field(name, value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Value::Map(self.entries))
    }
}

/// Wraps a tuple or struct variant's contents in a one-entry map keyed by the variant name,
/// serde's external tagging.
pub(super) struct VariantBuilder<B> {
    variant: &'static str,
    inner: B,
}

impl<B> VariantBuilder<B> {
    fn wrap(variant: &'static str, value: Value) -> Value {
        Value::Map(vec![(Value::Text(variant.to_owned()), value)])
    }
}

impl ser::SerializeTupleVariant for VariantBuilder<SeqBuilder> {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        self.inner.push(value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Self::wrap(self.variant, Value::Array(self.inner.0)))
    }
}

impl ser::SerializeStructVariant for VariantBuilder<MapBuilder> {
    type Ok = Value;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        self.inner.field(name, value)
    }
    fn end(self) -> Result<Value, EncodeError> {
        Ok(Self::wrap(self.variant, Value::Map(self.inner.entries)))
    }
}
