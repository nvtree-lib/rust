use std::fmt;
use std::io::{self, Read, Write};
use std::os::fd::RawFd;

mod transport;

pub const NVTREE_RO: u8 = 0x001;
pub const NVTREE_NODELETE: u8 = 0x002;
pub const NV_FLAG_IGNORE_CASE: u8 = 0x01;
pub const NV_FLAG_NO_UNIQUE: u8 = 0x02;

pub const NVTREE_BOOL: u16 = 0x010;
pub const NVTREE_NUMBER: u16 = 0x020;
pub const NVTREE_STRING: u16 = 0x040;
pub const NVTREE_BINARY: u16 = 0x400;
pub const NVTREE_DESCRIPTOR: u16 = 0x800;
pub const NVTREE_NULL: u16 = 0x080;
pub const NVTREE_SIMPLE: u16 = NVTREE_BOOL | NVTREE_NUMBER | NVTREE_STRING | NVTREE_NULL;

pub const NVTREE_ARRAY: u16 = 0x100;
pub const NVTREE_NESTED: u16 = 0x200;

const NVTREE_HEADER_MAGIC: u8 = 0x6c;
const NVTREE_HEADER_VERSION: u8 = 0x00;
const NVTREE_FLAG_LITTLE_ENDIAN: u8 = 0x00;
const NVTREE_FLAG_BIG_ENDIAN: u8 = 0x80;
const NV_FLAG_PUBLIC_MASK: u8 = 0x03;

const NV_TYPE_NULL: u8 = 1;
const NV_TYPE_BOOL: u8 = 2;
const NV_TYPE_NUMBER: u8 = 3;
const NV_TYPE_STRING: u8 = 4;
const NV_TYPE_NVLIST: u8 = 5;
const NV_TYPE_DESCRIPTOR: u8 = 6;
const NV_TYPE_BINARY: u8 = 7;
const NV_TYPE_BOOL_ARRAY: u8 = 8;
const NV_TYPE_NUMBER_ARRAY: u8 = 9;
const NV_TYPE_STRING_ARRAY: u8 = 10;
const NV_TYPE_NVLIST_ARRAY: u8 = 11;
const NV_TYPE_DESCRIPTOR_ARRAY: u8 = 12;
const NV_TYPE_NVLIST_ARRAY_NEXT: u8 = 254;
const NV_TYPE_END: u8 = 0xff;

const TREE_HEADER_LEN: usize = 19;
const PAIR_HEADER_LEN: usize = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    Little,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvtreeEndian {
    Native,
    Little,
    Big,
}

impl NvtreeEndian {
    fn byte_order(self) -> ByteOrder {
        match self {
            Self::Native => host_byte_order(),
            Self::Little => ByteOrder::Little,
            Self::Big => ByteOrder::Big,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvtreeError {
    BufferTooSmall,
    InvalidMagic,
    InvalidVersion,
    InvalidUtf8,
    InvalidName,
    UnsupportedType(u8),
    DescriptorMissing(usize),
    MissingName(String),
    TypeMismatch { expected: u8, actual: u8 },
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nvtvalue {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Binary(Vec<u8>),
    Descriptor(i32),
    Nested(Box<Nvtree>),
    BoolArray(Vec<bool>),
    NumberArray(Vec<u64>),
    StringArray(Vec<String>),
    NestedArray(Vec<Nvtree>),
    DescriptorArray(Vec<i32>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvtpair {
    pub flags: u8,
    pub name: String,
    pub value: Nvtvalue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NvtType {
    Null = NV_TYPE_NULL,
    Bool = NV_TYPE_BOOL,
    Number = NV_TYPE_NUMBER,
    String = NV_TYPE_STRING,
    Nested = NV_TYPE_NVLIST,
    Descriptor = NV_TYPE_DESCRIPTOR,
    Binary = NV_TYPE_BINARY,
    BoolArray = NV_TYPE_BOOL_ARRAY,
    NumberArray = NV_TYPE_NUMBER_ARRAY,
    StringArray = NV_TYPE_STRING_ARRAY,
    NestedArray = NV_TYPE_NVLIST_ARRAY,
    DescriptorArray = NV_TYPE_DESCRIPTOR_ARRAY,
}

impl Nvtvalue {
    pub fn nv_type(&self) -> NvtType {
        match self {
            Self::Null => NvtType::Null,
            Self::Bool(_) => NvtType::Bool,
            Self::Number(_) => NvtType::Number,
            Self::String(_) => NvtType::String,
            Self::Nested(_) => NvtType::Nested,
            Self::Descriptor(_) => NvtType::Descriptor,
            Self::Binary(_) => NvtType::Binary,
            Self::BoolArray(_) => NvtType::BoolArray,
            Self::NumberArray(_) => NvtType::NumberArray,
            Self::StringArray(_) => NvtType::StringArray,
            Self::NestedArray(_) => NvtType::NestedArray,
            Self::DescriptorArray(_) => NvtType::DescriptorArray,
        }
    }
}

impl Nvtpair {
    pub fn kind(&self) -> u16 {
        match self.value {
            Nvtvalue::Null => NVTREE_NULL,
            Nvtvalue::Bool(_) => NVTREE_BOOL,
            Nvtvalue::Number(_) => NVTREE_NUMBER,
            Nvtvalue::String(_) => NVTREE_STRING,
            Nvtvalue::Binary(_) => NVTREE_BINARY,
            Nvtvalue::Descriptor(_) => NVTREE_DESCRIPTOR,
            Nvtvalue::Nested(_) => NVTREE_NESTED,
            Nvtvalue::BoolArray(_) => NVTREE_ARRAY | NVTREE_BOOL,
            Nvtvalue::NumberArray(_) => NVTREE_ARRAY | NVTREE_NUMBER,
            Nvtvalue::StringArray(_) => NVTREE_ARRAY | NVTREE_STRING,
            Nvtvalue::NestedArray(_) => NVTREE_ARRAY | NVTREE_NESTED,
            Nvtvalue::DescriptorArray(_) => NVTREE_ARRAY | NVTREE_DESCRIPTOR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvtree {
    pub flags: u8,
    /// File descriptors received with this tree's ancillary data.
    ///
    /// Descriptors are not part of the serialized byte stream, so this is
    /// populated by the transport-aware receive path.
    pub descriptors: Vec<i32>,
    error: i32,
    head: Vec<Nvtpair>,
}

pub fn nvtree_create(flags: u8) -> Nvtree {
    Nvtree {
        flags,
        descriptors: Vec::new(),
        error: 0,
        head: Vec::new(),
    }
}

pub fn nvtree_pair(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Null,
    }
}

pub fn nvtree_number(name: &str, value: u64) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Number(value),
    }
}

pub fn nvtree_bool(name: &str, value: bool) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Bool(value),
    }
}

pub fn nvtree_string(name: &str, value: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::String(value.to_string()),
    }
}

pub fn nvtree_add_string_args(
    root: &mut Nvtree,
    name: &str,
    value: fmt::Arguments<'_>,
) -> Result<Option<Nvtpair>, NvtreeError> {
    Ok(nvtree_add(root, nvtree_string(name, &value.to_string())))
}

#[macro_export]
macro_rules! nvtree_add_stringf {
    ($root:expr, $name:expr, $($args:tt)*) => {
        $crate::nvtree_add_string_args($root, $name, format_args!($($args)*))
    };
}

pub fn nvtree_binary(name: &str, value: &[u8]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Binary(value.to_vec()),
    }
}

pub fn nvtree_descriptor(name: &str, value: i32) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Descriptor(value),
    }
}

pub fn nvtree_descriptor_array(name: &str, value: &[i32]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::DescriptorArray(value.to_vec()),
    }
}

pub fn nvtree_bool_array(name: &str, value: &[bool]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::BoolArray(value.to_vec()),
    }
}

pub fn nvtree_number_array(name: &str, value: &[u64]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::NumberArray(value.to_vec()),
    }
}

pub fn nvtree_string_array(name: &str, value: &[String]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::StringArray(value.to_vec()),
    }
}

pub fn nvtree_nested_array(name: &str, value: &[Nvtree]) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::NestedArray(value.to_vec()),
    }
}

pub fn nvtree_null(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Null,
    }
}

pub fn nvtree_tree(name: &str) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Nested(Box::new(nvtree_create(0))),
    }
}

pub fn nvtree_nested(name: &str, flags: u8) -> Nvtpair {
    Nvtpair {
        flags,
        name: name.to_string(),
        value: Nvtvalue::Nested(Box::new(nvtree_create(flags))),
    }
}

pub fn nvtree_move_string(name: &str, value: String) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::String(value),
    }
}

pub fn nvtree_move_nested(name: &str, value: Nvtree) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Nested(Box::new(value)),
    }
}

pub fn nvtree_move_binary(name: &str, value: Vec<u8>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::Binary(value),
    }
}

pub fn nvtree_move_bool_array(name: &str, value: Vec<bool>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::BoolArray(value),
    }
}

pub fn nvtree_move_number_array(name: &str, value: Vec<u64>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::NumberArray(value),
    }
}

pub fn nvtree_move_string_array(name: &str, value: Vec<String>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::StringArray(value),
    }
}

pub fn nvtree_move_nested_array(name: &str, value: Vec<Nvtree>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::NestedArray(value),
    }
}

pub fn nvtree_move_descriptor(name: &str, value: i32) -> Nvtpair {
    nvtree_descriptor(name, value)
}

pub fn nvtree_move_descriptor_array(name: &str, value: Vec<i32>) -> Nvtpair {
    Nvtpair {
        flags: 0,
        name: name.to_string(),
        value: Nvtvalue::DescriptorArray(value),
    }
}

pub fn nvtree_find<'a>(root: &'a Nvtree, name: &str) -> Option<&'a Nvtpair> {
    root.head
        .iter()
        .find(|pair| names_equal(root.flags, &pair.name, name))
}

pub fn nvtree_flags(root: &Nvtree) -> u8 {
    root.flags
}

pub fn nvtree_error(root: &Nvtree) -> i32 {
    root.error
}

pub fn nvtree_set_error(root: &mut Nvtree, error: i32) {
    root.error = error;
}

pub fn nvtree_set_flags(root: &mut Nvtree, flags: u8) -> Result<(), NvtreeError> {
    if flags & !NV_FLAG_PUBLIC_MASK != 0 {
        return Err(NvtreeError::Malformed);
    }
    root.flags = flags;
    Ok(())
}

pub fn nvtree_pairs(root: &Nvtree) -> impl Iterator<Item = &Nvtpair> {
    root.head.iter()
}

pub fn nvtree_next<'a>(root: &'a Nvtree, cursor: &mut usize) -> Option<(&'a str, NvtType)> {
    let pair = root.head.get(*cursor)?;
    *cursor += 1;
    Some((&pair.name, pair.value.nv_type()))
}

pub fn nvtree_iter(root: &Nvtree) -> impl Iterator<Item = (&str, NvtType)> {
    root.head
        .iter()
        .map(|pair| (pair.name.as_str(), pair.value.nv_type()))
}

fn dump_value<W: Write>(writer: &mut W, value: &Nvtvalue, indent: usize) -> io::Result<()> {
    match value {
        Nvtvalue::Null => write!(writer, "null"),
        Nvtvalue::Bool(value) => write!(writer, "{value}"),
        Nvtvalue::Number(value) => write!(writer, "{value}"),
        Nvtvalue::String(value) => write!(writer, "{:?}", value),
        Nvtvalue::Binary(value) => write!(writer, "binary({value:02x?})"),
        Nvtvalue::Descriptor(value) => write!(writer, "descriptor({value})"),
        Nvtvalue::Nested(value) => dump_tree(writer, value, indent),
        Nvtvalue::BoolArray(value) => write!(writer, "{value:?}"),
        Nvtvalue::NumberArray(value) => write!(writer, "{value:?}"),
        Nvtvalue::StringArray(value) => write!(writer, "{value:?}"),
        Nvtvalue::NestedArray(value) => {
            write!(writer, "[")?;
            for (index, child) in value.iter().enumerate() {
                if index != 0 {
                    write!(writer, ", ")?;
                }
                dump_tree(writer, child, indent + 1)?;
            }
            write!(writer, "]")
        }
        Nvtvalue::DescriptorArray(value) => write!(writer, "descriptor({value:?})"),
    }
}

fn dump_tree<W: Write>(writer: &mut W, root: &Nvtree, indent: usize) -> io::Result<()> {
    writeln!(writer, "{{")?;
    for pair in &root.head {
        for _ in 0..indent + 1 {
            write!(writer, "  ")?;
        }
        write!(writer, "{:?}: ", pair.name)?;
        dump_value(writer, &pair.value, indent + 1)?;
        writeln!(writer, ",")?;
    }
    for _ in 0..indent {
        write!(writer, "  ")?;
    }
    write!(writer, "}}")
}

pub fn nvtree_dump<W: Write>(root: &Nvtree, writer: &mut W) -> io::Result<()> {
    dump_tree(writer, root, 0)?;
    writeln!(writer)
}

pub fn nvtree_fdump<W: Write>(root: &Nvtree, writer: &mut W) -> io::Result<()> {
    nvtree_dump(root, writer)
}

pub fn nvtree_exists(root: &Nvtree, name: &str) -> bool {
    nvtree_find(root, name).is_some()
}

pub fn nvtree_exists_type(root: &Nvtree, name: &str, kind: NvtType) -> bool {
    nvtree_find(root, name).is_some_and(|pair| pair.value.nv_type() == kind)
}

pub fn nvtree_empty(root: &Nvtree) -> bool {
    root.head.is_empty()
}

pub fn nvtree_exists_null(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Null)
}

pub fn nvtree_exists_bool(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Bool)
}

pub fn nvtree_exists_number(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Number)
}

pub fn nvtree_exists_string(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::String)
}

pub fn nvtree_exists_nested(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Nested)
}

pub fn nvtree_exists_binary(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Binary)
}

pub fn nvtree_exists_bool_array(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::BoolArray)
}

pub fn nvtree_exists_number_array(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::NumberArray)
}

pub fn nvtree_exists_string_array(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::StringArray)
}

pub fn nvtree_exists_nested_array(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::NestedArray)
}

pub fn nvtree_exists_descriptor(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::Descriptor)
}

pub fn nvtree_exists_descriptor_array(root: &Nvtree, name: &str) -> bool {
    nvtree_exists_type(root, name, NvtType::DescriptorArray)
}

pub fn nvtree_get<'a>(root: &'a Nvtree, name: &str) -> Result<&'a Nvtvalue, NvtreeError> {
    nvtree_find(root, name)
        .map(|pair| &pair.value)
        .ok_or_else(|| NvtreeError::MissingName(name.to_string()))
}

fn nvtree_get_type<'a>(
    root: &'a Nvtree,
    name: &str,
    expected: NvtType,
) -> Result<&'a Nvtvalue, NvtreeError> {
    let value = nvtree_get(root, name)?;
    if value.nv_type() != expected {
        return Err(NvtreeError::TypeMismatch {
            expected: expected as u8,
            actual: value.nv_type() as u8,
        });
    }
    Ok(value)
}

pub fn nvtree_get_bool(root: &Nvtree, name: &str) -> Result<bool, NvtreeError> {
    match nvtree_get_type(root, name, NvtType::Bool)? {
        Nvtvalue::Bool(value) => Ok(*value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_null(root: &Nvtree, name: &str) -> Result<(), NvtreeError> {
    nvtree_get_type(root, name, NvtType::Null).map(|_| ())
}

pub fn nvtree_get_number(root: &Nvtree, name: &str) -> Result<u64, NvtreeError> {
    match nvtree_get_type(root, name, NvtType::Number)? {
        Nvtvalue::Number(value) => Ok(*value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_string<'a>(root: &'a Nvtree, name: &str) -> Result<&'a str, NvtreeError> {
    match nvtree_get_type(root, name, NvtType::String)? {
        Nvtvalue::String(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_binary<'a>(root: &'a Nvtree, name: &str) -> Result<&'a [u8], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::Binary)? {
        Nvtvalue::Binary(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_nested<'a>(root: &'a Nvtree, name: &str) -> Result<&'a Nvtree, NvtreeError> {
    match nvtree_get_type(root, name, NvtType::Nested)? {
        Nvtvalue::Nested(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_bool_array<'a>(root: &'a Nvtree, name: &str) -> Result<&'a [bool], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::BoolArray)? {
        Nvtvalue::BoolArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_number_array<'a>(root: &'a Nvtree, name: &str) -> Result<&'a [u64], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::NumberArray)? {
        Nvtvalue::NumberArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_string_array<'a>(
    root: &'a Nvtree,
    name: &str,
) -> Result<&'a [String], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::StringArray)? {
        Nvtvalue::StringArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_nested_array<'a>(
    root: &'a Nvtree,
    name: &str,
) -> Result<&'a [Nvtree], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::NestedArray)? {
        Nvtvalue::NestedArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_nested_array_len(root: &Nvtree, name: &str) -> Result<usize, NvtreeError> {
    Ok(nvtree_get_nested_array(root, name)?.len())
}

pub fn nvtree_get_nested_array_item<'a>(
    root: &'a Nvtree,
    name: &str,
    index: usize,
) -> Result<&'a Nvtree, NvtreeError> {
    nvtree_get_nested_array(root, name)?
        .get(index)
        .ok_or(NvtreeError::Malformed)
}

pub fn nvtree_get_array_next<'a>(
    root: &'a Nvtree,
    name: &str,
    cursor: &mut usize,
) -> Result<Option<&'a Nvtree>, NvtreeError> {
    let array = nvtree_get_nested_array(root, name)?;
    let item = array.get(*cursor);
    if item.is_some() {
        *cursor += 1;
    }
    Ok(item)
}

pub fn nvtree_get_descriptor(root: &Nvtree, name: &str) -> Result<i32, NvtreeError> {
    match nvtree_get_type(root, name, NvtType::Descriptor)? {
        Nvtvalue::Descriptor(value) => Ok(*value),
        _ => unreachable!(),
    }
}

pub fn nvtree_get_descriptor_array<'a>(
    root: &'a Nvtree,
    name: &str,
) -> Result<&'a [i32], NvtreeError> {
    match nvtree_get_type(root, name, NvtType::DescriptorArray)? {
        Nvtvalue::DescriptorArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take(root: &mut Nvtree, name: &str) -> Result<Nvtvalue, NvtreeError> {
    nvtree_remove(root, name)
        .map(|pair| pair.value)
        .ok_or_else(|| NvtreeError::MissingName(name.to_string()))
}

pub fn nvtree_take_type(
    root: &mut Nvtree,
    name: &str,
    kind: NvtType,
) -> Result<Nvtvalue, NvtreeError> {
    let value = nvtree_get(root, name)?;
    if value.nv_type() != kind {
        return Err(NvtreeError::TypeMismatch {
            expected: kind as u8,
            actual: value.nv_type() as u8,
        });
    }
    nvtree_take(root, name)
}

pub fn nvtree_take_null(root: &mut Nvtree, name: &str) -> Result<(), NvtreeError> {
    nvtree_take_type(root, name, NvtType::Null).map(|_| ())
}

pub fn nvtree_take_bool(root: &mut Nvtree, name: &str) -> Result<bool, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::Bool)? {
        Nvtvalue::Bool(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_number(root: &mut Nvtree, name: &str) -> Result<u64, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::Number)? {
        Nvtvalue::Number(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_string(root: &mut Nvtree, name: &str) -> Result<String, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::String)? {
        Nvtvalue::String(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_nested(root: &mut Nvtree, name: &str) -> Result<Nvtree, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::Nested)? {
        Nvtvalue::Nested(value) => Ok(*value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_binary(root: &mut Nvtree, name: &str) -> Result<Vec<u8>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::Binary)? {
        Nvtvalue::Binary(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_bool_array(root: &mut Nvtree, name: &str) -> Result<Vec<bool>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::BoolArray)? {
        Nvtvalue::BoolArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_number_array(root: &mut Nvtree, name: &str) -> Result<Vec<u64>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::NumberArray)? {
        Nvtvalue::NumberArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_string_array(root: &mut Nvtree, name: &str) -> Result<Vec<String>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::StringArray)? {
        Nvtvalue::StringArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_nested_array(root: &mut Nvtree, name: &str) -> Result<Vec<Nvtree>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::NestedArray)? {
        Nvtvalue::NestedArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_descriptor(root: &mut Nvtree, name: &str) -> Result<i32, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::Descriptor)? {
        Nvtvalue::Descriptor(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_take_descriptor_array(
    root: &mut Nvtree,
    name: &str,
) -> Result<Vec<i32>, NvtreeError> {
    match nvtree_take_type(root, name, NvtType::DescriptorArray)? {
        Nvtvalue::DescriptorArray(value) => Ok(value),
        _ => unreachable!(),
    }
}

pub fn nvtree_free(root: &mut Nvtree, name: &str) -> bool {
    nvtree_remove(root, name).is_some()
}

pub fn nvtree_free_type(root: &mut Nvtree, name: &str, kind: NvtType) -> bool {
    if nvtree_exists_type(root, name, kind) {
        nvtree_free(root, name)
    } else {
        false
    }
}

pub fn nvtree_free_null(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Null)
}

pub fn nvtree_free_bool(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Bool)
}

pub fn nvtree_free_number(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Number)
}

pub fn nvtree_free_string(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::String)
}

pub fn nvtree_free_nested(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Nested)
}

pub fn nvtree_free_binary(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Binary)
}

pub fn nvtree_free_bool_array(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::BoolArray)
}

pub fn nvtree_free_number_array(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::NumberArray)
}

pub fn nvtree_free_string_array(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::StringArray)
}

pub fn nvtree_free_nested_array(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::NestedArray)
}

pub fn nvtree_free_descriptor(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::Descriptor)
}

pub fn nvtree_free_descriptor_array(root: &mut Nvtree, name: &str) -> bool {
    nvtree_free_type(root, name, NvtType::DescriptorArray)
}

fn append_value(root: &mut Nvtree, name: &str, value: Nvtvalue) -> Result<(), NvtreeError> {
    let pair = nvtree_find(root, name).ok_or_else(|| NvtreeError::MissingName(name.to_string()))?;
    let mut replacement = pair.clone();
    match (&mut replacement.value, value) {
        (Nvtvalue::BoolArray(values), Nvtvalue::Bool(item)) => values.push(item),
        (Nvtvalue::NumberArray(values), Nvtvalue::Number(item)) => values.push(item),
        (Nvtvalue::StringArray(values), Nvtvalue::String(item)) => values.push(item),
        (Nvtvalue::NestedArray(values), Nvtvalue::Nested(item)) => values.push(*item),
        (Nvtvalue::DescriptorArray(values), Nvtvalue::Descriptor(item)) => values.push(item),
        (current, incoming) => {
            return Err(NvtreeError::TypeMismatch {
                expected: current.nv_type() as u8,
                actual: incoming.nv_type() as u8,
            });
        }
    }
    nvtree_add(root, replacement);
    Ok(())
}

pub fn nvtree_append_bool(root: &mut Nvtree, name: &str, value: bool) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Bool(value))
}

pub fn nvtree_append_number(root: &mut Nvtree, name: &str, value: u64) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Number(value))
}

pub fn nvtree_append_string(
    root: &mut Nvtree,
    name: &str,
    value: impl Into<String>,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::String(value.into()))
}

pub fn nvtree_append_nested(
    root: &mut Nvtree,
    name: &str,
    value: Nvtree,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Nested(Box::new(value)))
}

pub fn nvtree_append_descriptor(
    root: &mut Nvtree,
    name: &str,
    value: i32,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Descriptor(value))
}

pub fn nvtree_append_bool_array(
    root: &mut Nvtree,
    name: &str,
    value: bool,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Bool(value))
}

pub fn nvtree_append_number_array(
    root: &mut Nvtree,
    name: &str,
    value: u64,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Number(value))
}

pub fn nvtree_append_string_array(
    root: &mut Nvtree,
    name: &str,
    value: impl Into<String>,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::String(value.into()))
}

pub fn nvtree_append_nested_array(
    root: &mut Nvtree,
    name: &str,
    value: Nvtree,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Nested(Box::new(value)))
}

pub fn nvtree_append_descriptor_array(
    root: &mut Nvtree,
    name: &str,
    value: i32,
) -> Result<(), NvtreeError> {
    append_value(root, name, Nvtvalue::Descriptor(value))
}

pub fn nvtree_add(root: &mut Nvtree, pair: Nvtpair) -> Option<Nvtpair> {
    if root.flags & NV_FLAG_NO_UNIQUE == 0
        && let Some(existing) = root
            .head
            .iter_mut()
            .find(|item| names_equal(root.flags, &item.name, &pair.name))
    {
        return Some(std::mem::replace(existing, pair));
    }
    root.head.push(pair);
    None
}

pub fn nvtree_remove(root: &mut Nvtree, name: &str) -> Option<Nvtpair> {
    root.head
        .iter()
        .position(|pair| names_equal(root.flags, &pair.name, name))
        .map(|index| root.head.remove(index))
}

pub fn nvtree_add_tree(tree: &mut Nvtpair, pair: Nvtpair) -> Result<Option<Nvtpair>, NvtreeError> {
    match &mut tree.value {
        Nvtvalue::Nested(nested) => Ok(nvtree_add(nested, pair)),
        _ => Err(NvtreeError::Malformed),
    }
}

pub fn nvtree_rem_tree(tree: &mut Nvtpair, name: &str) -> Result<Option<Nvtpair>, NvtreeError> {
    match &mut tree.value {
        Nvtvalue::Nested(nested) => Ok(nvtree_remove(nested, name)),
        _ => Err(NvtreeError::Malformed),
    }
}

pub fn nvtree_size(root: &Nvtree) -> usize {
    nvtree_pack(root).map_or(0, |(bytes, _)| bytes.len())
}

/// Validate and pack an nvlist, returning its descriptor ancillary-data list
/// in wire order alongside the packed bytes.
pub fn nvtree_pack(root: &Nvtree) -> Result<(Vec<u8>, Vec<i32>), NvtreeError> {
    nvtree_pack_endian(root, NvtreeEndian::Native)
}

pub fn nvtree_pack_endian(
    root: &Nvtree,
    endian: NvtreeEndian,
) -> Result<(Vec<u8>, Vec<i32>), NvtreeError> {
    validate_tree(root)?;
    let mut state = PackState::default();
    let bytes = serialize_tree(root, true, &mut state, true, endian.byte_order());
    Ok((bytes, state.descriptors))
}

/// Unpack an nvlist from its serialized bytes.
///
/// Descriptor values are ancillary data and therefore cannot be recovered
/// from `buf` alone. Use the transport receive functions when the stream has
/// descriptor ancillary data.
pub fn nvtree_unpack(buf: &[u8]) -> Result<Nvtree, NvtreeError> {
    nvtree_unpack_with_descriptors(buf, &[])
}

fn nvtree_unpack_with_descriptors(buf: &[u8], descriptors: &[i32]) -> Result<Nvtree, NvtreeError> {
    let mut descriptor_index = 0;
    match parse_tree_internal(buf, 0, true, None, descriptors, &mut descriptor_index) {
        Ok((mut tree, consumed, _marker_end)) => {
            if consumed != buf.len() {
                return Err(NvtreeError::Malformed);
            }
            if descriptor_index != descriptors.len() {
                return Err(NvtreeError::Malformed);
            }
            tree.descriptors = descriptors.to_vec();
            Ok(tree)
        }
        Err(err) => Err(err),
    }
}

pub fn nvtree_send<W: Write>(writer: &mut W, root: &Nvtree) -> io::Result<()> {
    let (bytes, descriptors) = nvtree_pack(root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}")))?;
    if !descriptors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "descriptor ancillary transport must be supplied by the channel",
        ));
    }
    writer.write_all(&bytes)
}

pub fn nvtree_recv<R: Read>(reader: &mut R, descriptors: &[i32]) -> io::Result<Nvtree> {
    let mut header = [0u8; TREE_HEADER_LEN];
    reader.read_exact(&mut header)?;
    if header[0] != NVTREE_HEADER_MAGIC || header[1] != NVTREE_HEADER_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid nvlist header",
        ));
    }
    let byte_order = if header[2] & NVTREE_FLAG_BIG_ENDIAN != 0 {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    };
    let mut offset = 3;
    let _descriptor_count = read_u64(&header, &mut offset, byte_order)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid nvlist header"))?;
    let body_size = read_u64(&header, &mut offset, byte_order)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid nvlist header"))?;
    let body_size: usize = body_size
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "nvlist is too large"))?;
    let mut bytes = header.to_vec();
    bytes.resize(TREE_HEADER_LEN + body_size, 0);
    reader.read_exact(&mut bytes[TREE_HEADER_LEN..])?;
    nvtree_unpack_with_descriptors(&bytes, descriptors)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}")))
}

pub fn nvtree_xfer<S: Read + Write>(
    stream: &mut S,
    root: &Nvtree,
    descriptors: &[i32],
) -> io::Result<Nvtree> {
    let (bytes, packed_descriptors) = nvtree_pack(root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}")))?;
    if packed_descriptors != descriptors {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor ancillary data does not match nvlist",
        ));
    }
    stream.write_all(&bytes)?;
    stream.flush()?;
    nvtree_recv(stream, descriptors)
}

pub fn nvtree_send_fd(fd: RawFd, root: &Nvtree) -> io::Result<()> {
    let (bytes, descriptors) = nvtree_pack(root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}")))?;
    transport::send(fd, &bytes, &descriptors)
}

pub fn nvtree_recv_fd(fd: RawFd) -> io::Result<(Nvtree, Vec<RawFd>)> {
    let (bytes, descriptors) = transport::recv(fd)?;
    match nvtree_unpack_with_descriptors(&bytes, &descriptors) {
        Ok(tree) => Ok((tree, descriptors)),
        Err(error) => {
            for descriptor in descriptors {
                transport::close_fd(descriptor);
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{error:?}"),
            ))
        }
    }
}

pub fn nvtree_xfer_fd(fd: RawFd, root: &Nvtree) -> io::Result<(Nvtree, Vec<RawFd>)> {
    nvtree_send_fd(fd, root)?;
    nvtree_recv_fd(fd)
}

pub fn nvtree_destroy(_root: Nvtree) -> i32 {
    0
}

pub fn nvtree_clone(root: &Nvtree) -> Nvtree {
    root.clone()
}

fn validate_tree(tree: &Nvtree) -> Result<(), NvtreeError> {
    if tree.flags & !NV_FLAG_PUBLIC_MASK != 0 {
        return Err(NvtreeError::Malformed);
    }
    for pair in &tree.head {
        if pair.name.is_empty()
            || pair.name.as_bytes().contains(&0)
            || pair.name.len() + 1 > 2048
            || pair.name.len() + 1 > u16::MAX as usize
        {
            return Err(NvtreeError::InvalidName);
        }
        match &pair.value {
            Nvtvalue::String(value) if value.as_bytes().contains(&0) => {
                return Err(NvtreeError::InvalidUtf8);
            }
            Nvtvalue::Binary(value) if value.is_empty() => return Err(NvtreeError::Malformed),
            Nvtvalue::BoolArray(values) if values.is_empty() => return Err(NvtreeError::Malformed),
            Nvtvalue::NumberArray(values) if values.is_empty() => {
                return Err(NvtreeError::Malformed);
            }
            Nvtvalue::StringArray(values) if values.is_empty() => {
                return Err(NvtreeError::Malformed);
            }
            Nvtvalue::DescriptorArray(values) if values.is_empty() => {
                return Err(NvtreeError::Malformed);
            }
            Nvtvalue::Nested(value) => validate_tree(value)?,
            Nvtvalue::NestedArray(values) => {
                for value in values {
                    validate_tree(value)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Default)]
struct PackState {
    descriptors: Vec<i32>,
    include_descriptors: bool,
}

fn serialize_tree(
    tree: &Nvtree,
    root: bool,
    state: &mut PackState,
    include_descriptors: bool,
    byte_order: ByteOrder,
) -> Vec<u8> {
    state.include_descriptors = include_descriptors;
    let descriptor_start = state.descriptors.len();
    let mut body = Vec::new();
    for pair in &tree.head {
        serialize_pair(pair, &mut body, byte_order, state);
    }

    let total_size = TREE_HEADER_LEN + body.len();
    let size_field = if root {
        body.len() as u64
    } else {
        (total_size + 1) as u64
    };

    let mut out = Vec::with_capacity(total_size);
    out.push(NVTREE_HEADER_MAGIC);
    out.push(NVTREE_HEADER_VERSION);
    let flags_wire = (tree.flags & !NVTREE_FLAG_BIG_ENDIAN)
        | if byte_order == ByteOrder::Big {
            NVTREE_FLAG_BIG_ENDIAN
        } else {
            NVTREE_FLAG_LITTLE_ENDIAN
        };
    out.push(flags_wire);
    write_u64(
        &mut out,
        (state.descriptors.len() - descriptor_start) as u64,
        byte_order,
    );
    write_u64(&mut out, size_field, byte_order);
    out.extend_from_slice(&body);
    out
}

fn serialize_pair(pair: &Nvtpair, out: &mut Vec<u8>, byte_order: ByteOrder, state: &mut PackState) {
    let mut name = pair.name.as_bytes().to_vec();
    name.push(0);
    let namesize = u16::try_from(name.len()).unwrap_or(u16::MAX);

    match &pair.value {
        Nvtvalue::Null => {
            write_pair_header(out, NV_TYPE_NULL, namesize, 0, 0, byte_order);
            out.extend_from_slice(&name);
        }
        Nvtvalue::Bool(v) => {
            write_pair_header(out, NV_TYPE_BOOL, namesize, 1, 0, byte_order);
            out.extend_from_slice(&name);
            out.push(u8::from(*v));
        }
        Nvtvalue::Number(v) => {
            write_pair_header(out, NV_TYPE_NUMBER, namesize, 8, 0, byte_order);
            out.extend_from_slice(&name);
            write_u64(out, *v, byte_order);
        }
        Nvtvalue::String(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            write_pair_header(
                out,
                NV_TYPE_STRING,
                namesize,
                bytes.len() as u64,
                0,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
        Nvtvalue::Binary(bytes) => {
            write_pair_header(
                out,
                NV_TYPE_BINARY,
                namesize,
                bytes.len() as u64,
                0,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(bytes);
        }
        Nvtvalue::Descriptor(value) => {
            let wire = if *value == -1 || !state.include_descriptors {
                -1_i64
            } else {
                let index = state.descriptors.len() as i64;
                state.descriptors.push(*value);
                index
            };
            write_pair_header(out, NV_TYPE_DESCRIPTOR, namesize, 8, 0, byte_order);
            out.extend_from_slice(&name);
            write_u64(out, wire as u64, byte_order);
        }
        Nvtvalue::Nested(tree) => {
            let nested = serialize_tree(tree, false, state, state.include_descriptors, byte_order);
            write_pair_header(
                out,
                NV_TYPE_NVLIST,
                namesize,
                nested.len() as u64,
                0,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&nested);

            // C implementation appends an explicit END marker after each nested nvlist.
            write_pair_header(out, NV_TYPE_END, 1, 0, 0, byte_order);
            out.push(0);
        }
        Nvtvalue::BoolArray(values) => {
            write_pair_header(
                out,
                NV_TYPE_BOOL_ARRAY,
                namesize,
                values.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend(values.iter().map(|v| u8::from(*v)));
        }
        Nvtvalue::NumberArray(values) => {
            write_pair_header(
                out,
                NV_TYPE_NUMBER_ARRAY,
                namesize,
                (values.len() * 8) as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            for v in values {
                write_u64(out, *v, byte_order);
            }
        }
        Nvtvalue::StringArray(values) => {
            let mut bytes = Vec::new();
            for value in values {
                bytes.extend_from_slice(value.as_bytes());
                bytes.push(0);
            }
            write_pair_header(
                out,
                NV_TYPE_STRING_ARRAY,
                namesize,
                bytes.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
        Nvtvalue::NestedArray(values) => {
            let mut bytes = Vec::new();
            for value in values {
                bytes.extend_from_slice(&serialize_tree(
                    value,
                    false,
                    state,
                    state.include_descriptors,
                    byte_order,
                ));
                write_pair_header(&mut bytes, NV_TYPE_NVLIST_ARRAY_NEXT, 1, 0, 0, byte_order);
                bytes.push(0);
            }
            write_pair_header(
                out,
                NV_TYPE_NVLIST_ARRAY,
                namesize,
                bytes.len() as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(&bytes);
        }
        Nvtvalue::DescriptorArray(values) => {
            write_pair_header(
                out,
                NV_TYPE_DESCRIPTOR_ARRAY,
                namesize,
                (values.len() * 8) as u64,
                values.len() as u64,
                byte_order,
            );
            out.extend_from_slice(&name);
            for value in values {
                let wire = if *value == -1 || !state.include_descriptors {
                    -1_i64
                } else {
                    let index = state.descriptors.len() as i64;
                    state.descriptors.push(*value);
                    index
                };
                write_u64(out, wire as u64, byte_order);
            }
        }
    }
}

fn write_pair_header(
    out: &mut Vec<u8>,
    ty: u8,
    namesize: u16,
    datasize: u64,
    nitems: u64,
    byte_order: ByteOrder,
) {
    out.push(ty);
    write_u16(out, namesize, byte_order);
    write_u64(out, datasize, byte_order);
    write_u64(out, nitems, byte_order);
}

fn parse_tree_internal(
    buf: &[u8],
    start: usize,
    root: bool,
    bound: Option<usize>,
    descriptors: &[i32],
    descriptor_index: &mut usize,
) -> Result<(Nvtree, usize, Option<usize>), NvtreeError> {
    if buf.len().saturating_sub(start) < TREE_HEADER_LEN {
        return Err(NvtreeError::BufferTooSmall);
    }
    if let Some(bound) = bound
        && bound < start + TREE_HEADER_LEN
    {
        return Err(NvtreeError::Malformed);
    }

    let magic = buf[start];
    let version = buf[start + 1];
    let flags_wire = buf[start + 2];
    let byte_order = if (flags_wire & NVTREE_FLAG_BIG_ENDIAN) != 0 {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    };
    let flags = flags_wire & !NVTREE_FLAG_BIG_ENDIAN;

    if magic != NVTREE_HEADER_MAGIC {
        return Err(NvtreeError::InvalidMagic);
    }
    if version != NVTREE_HEADER_VERSION {
        return Err(NvtreeError::InvalidVersion);
    }
    if flags & !NV_FLAG_PUBLIC_MASK != 0 {
        return Err(NvtreeError::Malformed);
    }

    let mut off = start + 3;
    let descriptor_count = read_u64(buf, &mut off, byte_order)? as usize;
    let size = read_u64(buf, &mut off, byte_order)? as usize;
    if descriptor_count > descriptors.len().saturating_sub(*descriptor_index) {
        return Err(NvtreeError::DescriptorMissing(descriptor_count));
    }

    let mut body_len = if root {
        size
    } else {
        // FreeBSD nvlist streams are observed in two nested-size encodings:
        // - size includes header + body + trailing NUL
        // - size includes header + body
        // Accept both to interoperate with kernel/userland producers.
        size.checked_sub(TREE_HEADER_LEN + 1)
            .or_else(|| size.checked_sub(TREE_HEADER_LEN))
            .ok_or(NvtreeError::Malformed)?
    };

    let mut ptr = start + TREE_HEADER_LEN;
    let mut body_end = ptr.checked_add(body_len).ok_or(NvtreeError::Malformed)?;
    if let Some(bound) = bound {
        // Producers may declare a nested size that exceeds the pair datasize
        // (observed with FreeBSD kernel sndstat nvlists); the pair datasize is
        // authoritative for the tree's extent.
        if body_end > bound {
            body_end = bound;
            body_len = bound - ptr;
        }
    }
    if body_end > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }

    let mut tree = nvtree_create(flags);
    let mut descriptor_refs = 0usize;
    let mut marker_end: Option<usize> = None;

    while ptr < body_end {
        let ty = read_u8(buf, &mut ptr)?;
        let namesize = read_u16(buf, &mut ptr, byte_order)? as usize;
        let datasize = read_u64(buf, &mut ptr, byte_order)? as usize;
        let nitems = read_u64(buf, &mut ptr, byte_order)? as usize;
        if namesize == 0 || namesize > 2048 || ptr + namesize > body_end {
            return Err(NvtreeError::InvalidName);
        }

        if buf[ptr + namesize - 1] != 0 {
            return Err(NvtreeError::InvalidName);
        }
        let name_raw = &buf[ptr..ptr + namesize - 1];
        ptr += namesize;
        let name = std::str::from_utf8(name_raw)
            .map_err(|_| NvtreeError::InvalidUtf8)?
            .to_string();

        let pair = match ty {
            NV_TYPE_NULL => {
                if datasize != 0 {
                    return Err(NvtreeError::Malformed);
                }
                nvtree_null(&name)
            }
            NV_TYPE_BOOL => {
                if datasize != 1 || ptr + 1 > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let v = buf[ptr] != 0;
                if buf[ptr] > 1 {
                    return Err(NvtreeError::Malformed);
                }
                ptr += 1;
                nvtree_bool(&name, v)
            }
            NV_TYPE_NUMBER => {
                if datasize != 8 || ptr + 8 > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&buf[ptr..ptr + 8]);
                ptr += 8;
                let v = match byte_order {
                    ByteOrder::Little => u64::from_le_bytes(bytes),
                    ByteOrder::Big => u64::from_be_bytes(bytes),
                };
                nvtree_number(&name, v)
            }
            NV_TYPE_STRING => {
                if ptr + datasize > body_end || datasize == 0 {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if buf[ptr + datasize - 1] != 0 {
                    return Err(NvtreeError::Malformed);
                }
                let str_raw = &buf[ptr..ptr + datasize - 1];
                let s = std::str::from_utf8(str_raw).map_err(|_| NvtreeError::InvalidUtf8)?;
                ptr += datasize;
                nvtree_string(&name, s)
            }
            NV_TYPE_BINARY => {
                if datasize == 0 || ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let value = buf[ptr..ptr + datasize].to_vec();
                ptr += datasize;
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::Binary(value),
                }
            }
            NV_TYPE_DESCRIPTOR => {
                if datasize != 8 || ptr + 8 > body_end {
                    return Err(NvtreeError::Malformed);
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&buf[ptr..ptr + 8]);
                ptr += 8;
                let wire = match byte_order {
                    ByteOrder::Little => i64::from_le_bytes(bytes),
                    ByteOrder::Big => i64::from_be_bytes(bytes),
                };
                let value = if wire == -1 {
                    -1
                } else if wire < 0 {
                    return Err(NvtreeError::Malformed);
                } else {
                    let index = wire as usize;
                    let value = *descriptors
                        .get(index)
                        .ok_or(NvtreeError::DescriptorMissing(index))?;
                    descriptor_refs += 1;
                    *descriptor_index = (*descriptor_index).max(index + 1);
                    value
                };
                nvtree_descriptor(&name, value)
            }
            NV_TYPE_NVLIST => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let (nested, consumed, _marker_end) = parse_tree_internal(
                    buf,
                    ptr,
                    false,
                    Some(ptr + datasize),
                    descriptors,
                    descriptor_index,
                )?;
                if consumed > datasize {
                    return Err(NvtreeError::Malformed);
                }
                ptr += consumed;

                // Some streams encode nested size without marker/terminator bytes.
                // Consume any trailing bytes that are accounted for by datasize.
                if consumed < datasize {
                    let rem = datasize - consumed;
                    if rem == 1 && ptr < body_end {
                        ptr += 1;
                    } else if rem == PAIR_HEADER_LEN + 1
                        && ptr + rem <= body_end
                        && buf[ptr] == NV_TYPE_END
                    {
                        ptr += rem;
                    } else {
                        return Err(NvtreeError::Malformed);
                    }
                } else if ptr + PAIR_HEADER_LEN < body_end && buf[ptr] == NV_TYPE_END {
                    // C implementation may append an explicit END marker after nested nvlists.
                    ptr += PAIR_HEADER_LEN + 1;
                }

                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::Nested(Box::new(nested)),
                }
            }
            NV_TYPE_BOOL_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if nitems == 0 || datasize != nitems {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                for value in &buf[ptr..ptr + nitems] {
                    if *value > 1 {
                        return Err(NvtreeError::Malformed);
                    }
                    values.push(*value != 0);
                }
                ptr += datasize;
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::BoolArray(values),
                }
            }
            NV_TYPE_NUMBER_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if nitems == 0 || datasize != nitems.checked_mul(8).ok_or(NvtreeError::Malformed)? {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    let v = read_u64(buf, &mut ptr, byte_order)?;
                    values.push(v);
                }
                ptr += datasize - (nitems * 8);
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NumberArray(values),
                }
            }
            NV_TYPE_DESCRIPTOR_ARRAY => {
                if nitems == 0
                    || datasize != nitems.checked_mul(8).ok_or(NvtreeError::Malformed)?
                    || ptr + datasize > body_end
                {
                    return Err(NvtreeError::Malformed);
                }
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&buf[ptr..ptr + 8]);
                    ptr += 8;
                    let wire = match byte_order {
                        ByteOrder::Little => i64::from_le_bytes(bytes),
                        ByteOrder::Big => i64::from_be_bytes(bytes),
                    };
                    if wire == -1 {
                        values.push(-1);
                    } else if wire < 0 {
                        return Err(NvtreeError::Malformed);
                    } else {
                        let index = wire as usize;
                        descriptor_refs += 1;
                        values.push(
                            *descriptors
                                .get(index)
                                .ok_or(NvtreeError::DescriptorMissing(index))?,
                        );
                        *descriptor_index = (*descriptor_index).max(index + 1);
                    }
                }
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::DescriptorArray(values),
                }
            }
            NV_TYPE_STRING_ARRAY => {
                if ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                if nitems == 0 {
                    return Err(NvtreeError::Malformed);
                }
                let data_end = ptr + datasize;
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    let rest = &buf[ptr..data_end];
                    let rel_end = rest
                        .iter()
                        .position(|b| *b == 0)
                        .ok_or(NvtreeError::Malformed)?;
                    let s = std::str::from_utf8(&rest[..rel_end])
                        .map_err(|_| NvtreeError::InvalidUtf8)?;
                    values.push(s.to_string());
                    ptr += rel_end + 1;
                }
                if ptr != data_end {
                    return Err(NvtreeError::Malformed);
                }
                ptr = data_end;
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::StringArray(values),
                }
            }
            NV_TYPE_NVLIST_ARRAY => {
                if datasize > 0 && ptr + datasize > body_end {
                    return Err(NvtreeError::BufferTooSmall);
                }
                let data_end = if datasize > 0 {
                    ptr + datasize
                } else {
                    body_end
                };
                let mut values = Vec::with_capacity(nitems);
                for _ in 0..nitems {
                    if ptr >= data_end {
                        return Err(NvtreeError::Malformed);
                    }
                    let (nested, consumed, marker_end) = parse_tree_internal(
                        buf,
                        ptr,
                        false,
                        Some(data_end),
                        descriptors,
                        descriptor_index,
                    )?;
                    values.push(nested);
                    ptr = marker_end.unwrap_or_else(|| ptr + consumed);

                    // Some producers append explicit END markers between array elements.
                    if ptr + PAIR_HEADER_LEN < data_end
                        && (buf[ptr] == NV_TYPE_NVLIST_ARRAY_NEXT || buf[ptr] == NV_TYPE_END)
                    {
                        ptr += PAIR_HEADER_LEN + 1;
                    }
                }
                if datasize > 0 {
                    // Tolerate a single trailing terminator byte in bounded payloads.
                    if ptr + 1 == data_end && buf[ptr] == 0 {
                        ptr += 1;
                    }
                    if ptr != data_end {
                        return Err(NvtreeError::Malformed);
                    }
                }
                Nvtpair {
                    flags: 0,
                    name,
                    value: Nvtvalue::NestedArray(values),
                }
            }
            NV_TYPE_NVLIST_ARRAY_NEXT | NV_TYPE_END => {
                if root {
                    return Err(NvtreeError::Malformed);
                }
                // The terminator's name bytes belong to this tree. Some
                // producers (FreeBSD kernel sndstat nvlists) declare a size
                // that spans the rest of the buffer, so the position after
                // the terminator is the only reliable extent signal.
                marker_end = Some(ptr);
                break;
            }
            other => return Err(NvtreeError::UnsupportedType(other)),
        };

        if tree.flags & NV_FLAG_NO_UNIQUE == 0
            && tree
                .head
                .iter()
                .any(|item| names_equal(tree.flags, &item.name, &pair.name))
        {
            return Err(NvtreeError::Malformed);
        }
        tree.head.push(pair);
    }

    if descriptor_refs != descriptor_count {
        return Err(NvtreeError::Malformed);
    }

    Ok((tree, TREE_HEADER_LEN + body_len, marker_end))
}

fn names_equal(flags: u8, left: &str, right: &str) -> bool {
    if flags & NV_FLAG_IGNORE_CASE != 0 {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn read_u8(buf: &[u8], off: &mut usize) -> Result<u8, NvtreeError> {
    if *off + 1 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let v = buf[*off];
    *off += 1;
    Ok(v)
}

fn read_u16(buf: &[u8], off: &mut usize, byte_order: ByteOrder) -> Result<u16, NvtreeError> {
    if *off + 2 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&buf[*off..*off + 2]);
    *off += 2;
    Ok(match byte_order {
        ByteOrder::Little => u16::from_le_bytes(bytes),
        ByteOrder::Big => u16::from_be_bytes(bytes),
    })
}

fn read_u64(buf: &[u8], off: &mut usize, byte_order: ByteOrder) -> Result<u64, NvtreeError> {
    if *off + 8 > buf.len() {
        return Err(NvtreeError::BufferTooSmall);
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[*off..*off + 8]);
    *off += 8;
    Ok(match byte_order {
        ByteOrder::Little => u64::from_le_bytes(bytes),
        ByteOrder::Big => u64::from_be_bytes(bytes),
    })
}

fn write_u16(out: &mut Vec<u8>, value: u16, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn write_u64(out: &mut Vec<u8>, value: u64, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn host_byte_order() -> ByteOrder {
    if cfg!(target_endian = "big") {
        ByteOrder::Big
    } else {
        ByteOrder::Little
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvtree_pack(root: &Nvtree) -> Vec<u8> {
        super::nvtree_pack(root).unwrap().0
    }

    fn nvtree_unpack(buf: &[u8]) -> Result<Nvtree, NvtreeError> {
        super::nvtree_unpack(buf)
    }

    fn read_u16_raw(buf: &[u8], off: usize, is_be: bool) -> u16 {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(&buf[off..off + 2]);
        if is_be {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }

    fn read_u64_raw(buf: &[u8], off: usize, is_be: bool) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buf[off..off + 8]);
        if is_be {
            u64::from_be_bytes(bytes)
        } else {
            u64::from_le_bytes(bytes)
        }
    }

    fn write_u16_raw(buf: &mut [u8], off: usize, value: u16, is_be: bool) {
        let bytes = if is_be {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        buf[off..off + 2].copy_from_slice(&bytes);
    }

    fn write_u64_raw(buf: &mut [u8], off: usize, value: u64, is_be: bool) {
        let bytes = if is_be {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        buf[off..off + 8].copy_from_slice(&bytes);
    }

    fn rewrite_tree_endian(
        buf: &mut [u8],
        start: usize,
        src_is_be: bool,
        dst_is_be: bool,
        root: bool,
    ) -> Result<usize, ()> {
        if buf.len().saturating_sub(start) < TREE_HEADER_LEN {
            return Err(());
        }

        let flags = buf[start + 2] & !NVTREE_FLAG_BIG_ENDIAN;
        buf[start + 2] = flags | if dst_is_be { NVTREE_FLAG_BIG_ENDIAN } else { 0 };

        let desc = read_u64_raw(buf, start + 3, src_is_be);
        let size = read_u64_raw(buf, start + 11, src_is_be);
        write_u64_raw(buf, start + 3, desc, dst_is_be);
        write_u64_raw(buf, start + 11, size, dst_is_be);

        let body_len = if root {
            size as usize
        } else {
            size.checked_sub((TREE_HEADER_LEN + 1) as u64).ok_or(())? as usize
        };
        let mut ptr = start + TREE_HEADER_LEN;
        let body_end = ptr.checked_add(body_len).ok_or(())?;
        if body_end > buf.len() {
            return Err(());
        }

        while ptr < body_end {
            let ty = buf[ptr];
            let namesize = read_u16_raw(buf, ptr + 1, src_is_be);
            let datasize = read_u64_raw(buf, ptr + 3, src_is_be);
            let nitems = read_u64_raw(buf, ptr + 11, src_is_be);
            write_u16_raw(buf, ptr + 1, namesize, dst_is_be);
            write_u64_raw(buf, ptr + 3, datasize, dst_is_be);
            write_u64_raw(buf, ptr + 11, nitems, dst_is_be);

            ptr += PAIR_HEADER_LEN;
            let ns = namesize as usize;
            if ns == 0 || ptr + ns > body_end {
                return Err(());
            }
            ptr += ns;

            match ty {
                NV_TYPE_NULL => {}
                NV_TYPE_BOOL => {
                    if ptr + 1 > body_end {
                        return Err(());
                    }
                    ptr += 1;
                }
                NV_TYPE_NUMBER => {
                    if ptr + 8 > body_end {
                        return Err(());
                    }
                    let num = read_u64_raw(buf, ptr, src_is_be);
                    write_u64_raw(buf, ptr, num, dst_is_be);
                    ptr += 8;
                }
                NV_TYPE_STRING => {
                    let ds = datasize as usize;
                    if ptr + ds > body_end {
                        return Err(());
                    }
                    ptr += ds;
                }
                NV_TYPE_BINARY => {
                    let ds = datasize as usize;
                    if ptr + ds > body_end {
                        return Err(());
                    }
                    ptr += ds;
                }
                NV_TYPE_NVLIST => {
                    let ds = datasize as usize;
                    let consumed = rewrite_tree_endian(buf, ptr, src_is_be, dst_is_be, false)?;
                    if consumed != ds {
                        return Err(());
                    }
                    ptr += consumed;
                    if ptr + PAIR_HEADER_LEN < body_end && buf[ptr] == NV_TYPE_END {
                        let marker_namesize = read_u16_raw(buf, ptr + 1, src_is_be);
                        let marker_datasize = read_u64_raw(buf, ptr + 3, src_is_be);
                        let marker_nitems = read_u64_raw(buf, ptr + 11, src_is_be);
                        write_u16_raw(buf, ptr + 1, marker_namesize, dst_is_be);
                        write_u64_raw(buf, ptr + 3, marker_datasize, dst_is_be);
                        write_u64_raw(buf, ptr + 11, marker_nitems, dst_is_be);
                        ptr += PAIR_HEADER_LEN + 1;
                    }
                }
                NV_TYPE_END => break,
                _ => return Err(()),
            }
        }

        Ok(TREE_HEADER_LEN + body_len)
    }

    #[test]
    fn nvtree_create_test() {
        let root = nvtree_create(0);
        assert_eq!(root.flags, 0);
        assert!(root.head.is_empty());
    }

    #[test]
    fn nvtree_find_test() {
        let name = "number";
        let mut root = nvtree_create(0);

        assert!(nvtree_add(&mut root, nvtree_number(name, 5)).is_none());
        assert!(nvtree_find(&root, name).is_some());
        assert!(nvtree_find(&root, "missing").is_none());
    }

    #[test]
    fn pair_kinds_cover_all_types() {
        assert_eq!(nvtree_null("n").kind(), NVTREE_NULL);
        assert_eq!(nvtree_bool("b", true).kind(), NVTREE_BOOL);
        assert_eq!(nvtree_number("u", 7).kind(), NVTREE_NUMBER);
        assert_eq!(nvtree_string("s", "x").kind(), NVTREE_STRING);
        assert_eq!(nvtree_tree("t").kind(), NVTREE_NESTED);
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "ba".to_string(),
                value: Nvtvalue::BoolArray(vec![true, false]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_BOOL
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "na".to_string(),
                value: Nvtvalue::NumberArray(vec![1, 2]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_NUMBER
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "sa".to_string(),
                value: Nvtvalue::StringArray(vec!["a".to_string(), "b".to_string()]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_STRING
        );
        assert_eq!(
            Nvtpair {
                flags: 0,
                name: "ta".to_string(),
                value: Nvtvalue::NestedArray(vec![nvtree_create(0)]),
            }
            .kind(),
            NVTREE_ARRAY | NVTREE_NESTED
        );
    }

    #[test]
    fn nvtree_pack_roundtrip_all_scalar_types() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_null("null"));
        nvtree_add(&mut root, nvtree_bool("bool", true));
        nvtree_add(&mut root, nvtree_number("number", 5));
        nvtree_add(&mut root, nvtree_string("string", "hello"));

        let buf = nvtree_pack(&root);
        assert!(!buf.is_empty());

        let unpacked = nvtree_unpack(&buf).expect("unpack should succeed");
        assert_eq!(
            nvtree_find(&unpacked, "null")
                .expect("null key should exist")
                .value,
            Nvtvalue::Null
        );
        assert_eq!(
            nvtree_find(&unpacked, "bool")
                .expect("bool key should exist")
                .value,
            Nvtvalue::Bool(true)
        );
        assert_eq!(
            nvtree_find(&unpacked, "number")
                .expect("number key should exist")
                .value,
            Nvtvalue::Number(5)
        );
        assert_eq!(
            nvtree_find(&unpacked, "string")
                .expect("string key should exist")
                .value,
            Nvtvalue::String("hello".to_string())
        );
    }

    #[test]
    fn nvtree_pack_roundtrip_array_types() {
        let mut root = nvtree_create(0);
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "bools".to_string(),
                value: Nvtvalue::BoolArray(vec![true, false, true]),
            },
        );
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "numbers".to_string(),
                value: Nvtvalue::NumberArray(vec![7, 11]),
            },
        );
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "strings".to_string(),
                value: Nvtvalue::StringArray(vec!["a".to_string(), "bc".to_string()]),
            },
        );
        let mut child = nvtree_create(0);
        nvtree_add(&mut child, nvtree_string("name", "inner"));
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "trees".to_string(),
                value: Nvtvalue::NestedArray(vec![child]),
            },
        );

        let packed = nvtree_pack(&root);
        let unpacked = nvtree_unpack(&packed).expect("array unpack should succeed");

        assert_eq!(
            nvtree_find(&unpacked, "bools")
                .expect("bool array key should exist")
                .value,
            Nvtvalue::BoolArray(vec![true, false, true])
        );
        assert_eq!(
            nvtree_find(&unpacked, "numbers")
                .expect("number array key should exist")
                .value,
            Nvtvalue::NumberArray(vec![7, 11])
        );
        assert_eq!(
            nvtree_find(&unpacked, "strings")
                .expect("string array key should exist")
                .value,
            Nvtvalue::StringArray(vec!["a".to_string(), "bc".to_string()])
        );
        let trees = nvtree_find(&unpacked, "trees").expect("nested array key should exist");
        match &trees.value {
            Nvtvalue::NestedArray(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(
                    nvtree_find(&values[0], "name")
                        .expect("nested tree item should contain name")
                        .value,
                    Nvtvalue::String("inner".to_string())
                );
            }
            _ => panic!("trees should be nested array"),
        }
    }

    #[test]
    fn nvtree_pack_roundtrip_nested_type() {
        let mut root = nvtree_create(0);
        let mut nested = nvtree_tree("child");
        nvtree_add_tree(&mut nested, nvtree_bool("ok", true)).expect("nested add must work");
        nvtree_add_tree(&mut nested, nvtree_string("name", "inner")).expect("nested add must work");
        nvtree_add(&mut root, nested);

        let packed = nvtree_pack(&root);
        let unpacked = nvtree_unpack(&packed).expect("unpack should succeed");

        let child = nvtree_find(&unpacked, "child").expect("child should exist");
        match &child.value {
            Nvtvalue::Nested(tree) => {
                let ok = nvtree_find(tree, "ok").expect("ok should exist in nested tree");
                assert_eq!(ok.value, Nvtvalue::Bool(true));
                let name = nvtree_find(tree, "name").expect("name should exist in nested tree");
                assert_eq!(name.value, Nvtvalue::String("inner".to_string()));
            }
            _ => panic!("child should be nested"),
        }
    }

    #[test]
    fn nvtree_remove_and_destroy_test() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_number("number", 42));
        assert!(nvtree_remove(&mut root, "number").is_some());
        assert!(nvtree_find(&root, "number").is_none());
        assert_eq!(nvtree_destroy(root), 0);
    }

    #[test]
    fn nvtree_unpack_accepts_endian_flagged_opposite_encoding() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_null("null"));
        nvtree_add(&mut root, nvtree_bool("bool", true));
        nvtree_add(&mut root, nvtree_number("number", 12345));
        nvtree_add(&mut root, nvtree_string("string", "endian"));
        let mut child = nvtree_tree("child");
        nvtree_add_tree(&mut child, nvtree_number("n", 7)).expect("nested add must work");
        nvtree_add(&mut root, child);

        let packed = nvtree_pack(&root);
        let mut flipped = packed.clone();
        let src_is_be = cfg!(target_endian = "big");
        let dst_is_be = !src_is_be;
        rewrite_tree_endian(&mut flipped, 0, src_is_be, dst_is_be, true)
            .expect("endianness rewrite should succeed");

        let unpacked = nvtree_unpack(&flipped).expect("unpack should succeed with endian marker");
        assert_eq!(
            nvtree_find(&unpacked, "number")
                .expect("number key should exist")
                .value,
            Nvtvalue::Number(12345)
        );
        assert_eq!(
            nvtree_find(&unpacked, "string")
                .expect("string key should exist")
                .value,
            Nvtvalue::String("endian".to_string())
        );
        let child = nvtree_find(&unpacked, "child").expect("child should exist");
        match &child.value {
            Nvtvalue::Nested(tree) => {
                assert_eq!(
                    nvtree_find(tree, "n").expect("nested n should exist").value,
                    Nvtvalue::Number(7)
                );
            }
            _ => panic!("child should be nested"),
        }
    }

    #[test]
    fn explicit_endian_pack_roundtrips() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_number("number", 0x0102_0304_0506_0708));
        nvtree_add(&mut root, nvtree_number_array("numbers", &[1, 2, 3]));

        for endian in [NvtreeEndian::Little, NvtreeEndian::Big] {
            let (packed, descriptors) = super::nvtree_pack_endian(&root, endian).unwrap();
            let decoded = super::nvtree_unpack_with_descriptors(&packed, &descriptors).unwrap();
            assert_eq!(decoded, root);
        }
    }

    #[test]
    fn stream_transport_roundtrips_without_descriptors() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_string("message", "hello"));
        let mut stream = std::io::Cursor::new(Vec::new());
        super::nvtree_send(&mut stream, &root).unwrap();
        stream.set_position(0);
        let decoded = super::nvtree_recv(&mut stream, &[]).unwrap();
        assert_eq!(decoded, root);
    }

    #[test]
    fn nvtree_unpack_accepts_nvlist_array_with_zero_datasize() {
        let mut root = nvtree_create(0);
        let mut item = nvtree_create(0);
        nvtree_add(&mut item, nvtree_string("devnode", "dsp0"));
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "dsps".to_string(),
                value: Nvtvalue::NestedArray(vec![item]),
            },
        );

        let mut packed = nvtree_pack(&root);
        let is_be = (packed[2] & NVTREE_FLAG_BIG_ENDIAN) != 0;
        // First/root pair starts immediately after tree header.
        let pair_off = TREE_HEADER_LEN;
        assert_eq!(packed[pair_off], NV_TYPE_NVLIST_ARRAY);
        write_u64_raw(&mut packed, pair_off + 3, 0, is_be);

        let unpacked = nvtree_unpack(&packed).expect("should unpack zero-datasize nvlist array");
        let dsps = nvtree_find(&unpacked, "dsps").expect("dsps key should exist");
        match &dsps.value {
            Nvtvalue::NestedArray(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(
                    nvtree_find(&values[0], "devnode")
                        .expect("devnode key should exist")
                        .value,
                    Nvtvalue::String("dsp0".to_string())
                );
            }
            _ => panic!("dsps should be nvlist array"),
        }
    }

    #[test]
    fn descriptor_pack_and_unpack_uses_ancillary_order() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_descriptor("fd", 42));
        nvtree_add(&mut root, nvtree_descriptor_array("fds", &[-1, 99]));

        let (packed, descriptors) = super::nvtree_pack(&root).unwrap();
        assert_eq!(descriptors, vec![42, 99]);
        let unpacked = super::nvtree_unpack_with_descriptors(&packed, &descriptors).unwrap();
        assert_eq!(
            nvtree_find(&unpacked, "fd").unwrap().value,
            Nvtvalue::Descriptor(42)
        );
        assert_eq!(
            nvtree_find(&unpacked, "fds").unwrap().value,
            Nvtvalue::DescriptorArray(vec![-1, 99])
        );
    }

    #[test]
    fn flags_control_case_and_duplicate_names() {
        let mut root = nvtree_create(NV_FLAG_IGNORE_CASE);
        nvtree_add(&mut root, nvtree_number("Name", 1));
        assert_eq!(
            nvtree_find(&root, "name").unwrap().value,
            Nvtvalue::Number(1)
        );
        assert_eq!(
            nvtree_add(&mut root, nvtree_number("NAME", 2))
                .unwrap()
                .value,
            Nvtvalue::Number(1)
        );

        let mut duplicate = nvtree_create(NV_FLAG_NO_UNIQUE);
        nvtree_add(&mut duplicate, nvtree_number("name", 1));
        nvtree_add(&mut duplicate, nvtree_number("name", 2));
        let packed = nvtree_pack(&duplicate);
        let unpacked = nvtree_unpack(&packed).unwrap();
        assert_eq!(unpacked.head.len(), 2);
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_bool("ok", true));
        let mut packed = nvtree_pack(&root);
        packed[41] = 2;
        assert!(matches!(
            nvtree_unpack(&packed),
            Err(NvtreeError::Malformed)
        ));

        let mut descriptor_root = nvtree_create(0);
        nvtree_add(&mut descriptor_root, nvtree_descriptor("fd", 7));
        let (packed, _) = super::nvtree_pack(&descriptor_root).unwrap();
        assert!(matches!(
            nvtree_unpack(&packed),
            Err(NvtreeError::DescriptorMissing(_))
        ));
    }

    struct ChunkedReader {
        bytes: Vec<u8>,
        offset: usize,
        chunk_size: usize,
    }

    struct ChunkedWriter {
        bytes: Vec<u8>,
        chunk_size: usize,
    }

    impl std::io::Write for ChunkedWriter {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            let count = self.chunk_size.min(input.len());
            self.bytes.extend_from_slice(&input[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FailingReader {
        bytes: Vec<u8>,
        offset: usize,
        fail_after: usize,
    }

    impl std::io::Read for FailingReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if self.offset >= self.fail_after {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "injected read failure",
                ));
            }
            let count = output
                .len()
                .min(self.fail_after - self.offset)
                .min(self.bytes.len() - self.offset);
            output[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }

    struct FailingWriter {
        bytes: Vec<u8>,
        remaining: usize,
    }

    impl std::io::Write for FailingWriter {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "injected write failure",
                ));
            }
            let count = self.remaining.min(input.len());
            self.bytes.extend_from_slice(&input[..count]);
            self.remaining -= count;
            Ok(count)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Duplex {
        response: Vec<u8>,
        read_offset: usize,
        request: Vec<u8>,
    }

    impl std::io::Read for Duplex {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if self.read_offset == self.response.len() {
                return Ok(0);
            }
            let count = output
                .len()
                .min(2)
                .min(self.response.len() - self.read_offset);
            output[..count]
                .copy_from_slice(&self.response[self.read_offset..self.read_offset + count]);
            self.read_offset += count;
            Ok(count)
        }
    }

    impl std::io::Write for Duplex {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            self.request.extend_from_slice(input);
            Ok(input.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl std::io::Read for ChunkedReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if self.offset == self.bytes.len() {
                return Ok(0);
            }
            let count = self
                .chunk_size
                .min(output.len())
                .min(self.bytes.len() - self.offset);
            output[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }

    #[test]
    fn typed_operations_iteration_and_nested_array_navigation_work() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_bool("flag", true));
        nvtree_add(&mut root, nvtree_number_array("numbers", &[1, 2]));
        nvtree_add(
            &mut root,
            nvtree_nested_array("children", &[nvtree_create(0), nvtree_create(0)]),
        );

        assert!(nvtree_exists_bool(&root, "flag"));
        assert_eq!(nvtree_nested_array_len(&root, "children").unwrap(), 2);
        assert!(nvtree_get_nested_array_item(&root, "children", 1).is_ok());
        let mut cursor = 0;
        assert!(
            nvtree_get_array_next(&root, "children", &mut cursor)
                .unwrap()
                .is_some()
        );
        assert!(
            nvtree_get_array_next(&root, "children", &mut cursor)
                .unwrap()
                .is_some()
        );
        assert!(
            nvtree_get_array_next(&root, "children", &mut cursor)
                .unwrap()
                .is_none()
        );

        let names: Vec<_> = nvtree_iter(&root).map(|(name, _)| name).collect();
        assert_eq!(names, vec!["flag", "numbers", "children"]);
        assert_eq!(nvtree_take_bool(&mut root, "flag"), Ok(true));
        assert!(nvtree_free_number_array(&mut root, "numbers"));
        assert!(!nvtree_empty(&root));
        assert!(nvtree_free_nested_array(&mut root, "children"));
        assert!(nvtree_empty(&root));
    }

    #[test]
    fn formatting_dump_and_error_state_are_available_without_ffi() {
        let mut root = nvtree_create(0);
        nvtree_add_stringf!(&mut root, "message", "{} {}", "hello", 7).unwrap();
        let mut dump = Vec::new();
        nvtree_dump(&root, &mut dump).unwrap();
        let dump = String::from_utf8(dump).unwrap();
        assert!(dump.contains("message"));
        assert!(dump.contains("hello 7"));

        assert_eq!(nvtree_error(&root), 0);
        nvtree_set_error(&mut root, 22);
        assert_eq!(nvtree_error(&root), 22);
    }

    #[test]
    fn fragmented_stream_reads_and_descriptor_sentinels_roundtrip() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_descriptor("missing", -1));
        nvtree_add(&mut root, nvtree_descriptor_array("fds", &[-1, -1]));
        let (bytes, descriptors) = super::nvtree_pack(&root).unwrap();
        assert!(descriptors.is_empty());

        let mut reader = ChunkedReader {
            bytes,
            offset: 0,
            chunk_size: 3,
        };
        let decoded = super::nvtree_recv(&mut reader, &[]).unwrap();
        assert_eq!(decoded, root);
    }

    #[test]
    fn validation_rejects_invalid_flags_names_and_payload_sizes() {
        let mut invalid_flags = nvtree_create(0x80);
        nvtree_add(&mut invalid_flags, nvtree_bool("ok", true));
        assert!(matches!(
            super::nvtree_pack(&invalid_flags),
            Err(NvtreeError::Malformed)
        ));

        let mut invalid_name = nvtree_create(0);
        nvtree_add(&mut invalid_name, nvtree_bool("bad\0name", true));
        assert!(matches!(
            super::nvtree_pack(&invalid_name),
            Err(NvtreeError::InvalidName)
        ));

        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_number("value", 1));
        let mut bytes = nvtree_pack(&root);
        let is_be = bytes[2] & NVTREE_FLAG_BIG_ENDIAN != 0;
        write_u64_raw(&mut bytes, 11, u64::MAX, is_be);
        assert!(nvtree_unpack(&bytes).is_err());
    }

    #[test]
    fn deterministic_property_roundtrips_cover_values_and_both_endians() {
        for seed in 0..64u64 {
            let mut root = nvtree_create(if seed % 2 == 0 {
                0
            } else {
                NV_FLAG_IGNORE_CASE
            });
            nvtree_add(&mut root, nvtree_bool("bool", seed % 2 == 0));
            nvtree_add(&mut root, nvtree_number("number", seed.wrapping_mul(7919)));
            nvtree_add(&mut root, nvtree_string("string", &format!("value-{seed}")));
            nvtree_add(
                &mut root,
                nvtree_number_array("numbers", &[seed, seed + 1, seed + 2]),
            );
            let (little, little_fds) = nvtree_pack_endian(&root, NvtreeEndian::Little).unwrap();
            let (big, big_fds) = nvtree_pack_endian(&root, NvtreeEndian::Big).unwrap();
            assert!(little_fds.is_empty() && big_fds.is_empty());
            assert_eq!(nvtree_unpack(&little).unwrap(), root);
            assert_eq!(nvtree_unpack(&big).unwrap(), root);
        }
    }

    #[test]
    fn stream_write_xfer_and_injected_io_errors_are_handled() {
        let mut root = nvtree_create(0);
        nvtree_add(&mut root, nvtree_string("message", "partial writes"));

        let mut writer = ChunkedWriter {
            bytes: Vec::new(),
            chunk_size: 2,
        };
        nvtree_send(&mut writer, &root).unwrap();
        assert_eq!(nvtree_unpack(&writer.bytes).unwrap(), root);

        let mut failing_writer = FailingWriter {
            bytes: Vec::new(),
            remaining: 3,
        };
        assert!(nvtree_send(&mut failing_writer, &root).is_err());

        let response = nvtree_pack(&root);
        let mut duplex = Duplex {
            response,
            read_offset: 0,
            request: Vec::new(),
        };
        assert_eq!(nvtree_xfer(&mut duplex, &root, &[]).unwrap(), root);
        assert_eq!(nvtree_unpack(&duplex.request).unwrap(), root);

        let mut failing_reader = FailingReader {
            bytes: writer.bytes,
            offset: 0,
            fail_after: 5,
        };
        assert!(nvtree_recv(&mut failing_reader, &[]).is_err());
    }

    #[test]
    fn empty_values_and_descriptor_indexes_are_rejected_or_preserved() {
        let mut root = nvtree_create(0);
        nvtree_add(
            &mut root,
            Nvtpair {
                flags: 0,
                name: "empty-binary".to_string(),
                value: Nvtvalue::Binary(Vec::new()),
            },
        );
        assert!(matches!(
            super::nvtree_pack(&root),
            Err(NvtreeError::Malformed)
        ));

        let mut descriptors = nvtree_create(0);
        nvtree_add(&mut descriptors, nvtree_descriptor("first", 11));
        nvtree_add(&mut descriptors, nvtree_descriptor("second", 22));
        let (bytes, fds) = super::nvtree_pack(&descriptors).unwrap();
        assert_eq!(fds, vec![11, 22]);
        assert_eq!(
            super::nvtree_unpack_with_descriptors(&bytes, &[101, 202]).unwrap(),
            Nvtree {
                flags: 0,
                descriptors: vec![101, 202],
                error: 0,
                head: vec![
                    nvtree_descriptor("first", 101),
                    nvtree_descriptor("second", 202)
                ],
            }
        );
        assert!(matches!(
            super::nvtree_unpack_with_descriptors(&bytes, &[101]),
            Err(NvtreeError::DescriptorMissing(_))
        ));
    }

    #[test]
    fn all_dump_value_kinds_and_duplicate_flag_combinations_are_covered() {
        let mut root = nvtree_create(NV_FLAG_IGNORE_CASE | NV_FLAG_NO_UNIQUE);
        nvtree_add(&mut root, nvtree_null("null"));
        nvtree_add(&mut root, nvtree_bool("bool", false));
        nvtree_add(&mut root, nvtree_number("number", 9));
        nvtree_add(&mut root, nvtree_string("string", "text"));
        nvtree_add(&mut root, nvtree_binary("binary", &[1, 2]));
        nvtree_add(&mut root, nvtree_descriptor("descriptor", -1));
        nvtree_add(&mut root, nvtree_bool_array("bool-array", &[true, false]));
        nvtree_add(&mut root, nvtree_number_array("number-array", &[1, 2]));
        nvtree_add(
            &mut root,
            nvtree_string_array("string-array", &["a".to_string(), "b".to_string()]),
        );
        nvtree_add(
            &mut root,
            nvtree_nested_array("nested-array", &[nvtree_create(0)]),
        );
        nvtree_add(
            &mut root,
            nvtree_descriptor_array("descriptor-array", &[-1]),
        );
        nvtree_add(&mut root, nvtree_number("DUP", 1));
        nvtree_add(&mut root, nvtree_number("dup", 2));
        assert_eq!(root.head.len(), 13);
        assert_eq!(
            nvtree_find(&root, "DuP").unwrap().value,
            Nvtvalue::Number(1)
        );

        let mut output = Vec::new();
        nvtree_fdump(&root, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        for name in [
            "null",
            "bool",
            "number",
            "string",
            "binary",
            "descriptor",
            "bool-array",
            "number-array",
            "string-array",
            "nested-array",
            "descriptor-array",
        ] {
            assert!(output.contains(name), "dump omitted {name}");
        }
    }
}
