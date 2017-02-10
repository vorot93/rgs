extern crate std;
extern crate byteorder;

use std::io::Cursor;
use self::byteorder::{BigEndian, ReadBytesExt};
use errors::Error;

macro_rules! try_next {
    ($iter:expr) => { try!($iter.next().ok_or(Error::InvalidPacketError("Early EOF while parsing packet".into()))) }
}

macro_rules! repeat_to_array {
    (($iter:expr),*) => {}
}

macro_rules! items {
    ($($item:item)*) => ($($item)*);
}

macro_rules! trait_alias {
    ($name:ident = $($base:tt)+) => {
        items! {
            pub trait $name: $($base)+ { }
            impl<T: $($base)+> $name for T { }
        }
    };
}

pub fn to_u16(slice: &[u8; 2]) -> u16 {
    Cursor::new(slice).read_u16::<BigEndian>().unwrap()
}

pub fn to_u32(slice: &[u8; 4]) -> u32 {
    Cursor::new(slice).read_u32::<BigEndian>().unwrap()
}

pub fn multi_next<'a, V, I: Iterator<Item = &'a V>>(iter: &mut I,
                                                    n: i64)
                                                    -> Result<Box<[&'a V]>, Error> {
    let mut v = Vec::new();
    for _ in 0..n {
        v.push(try_next!(iter));
    }

    Ok(v.into_boxed_slice())
}

pub fn read_string<'a, I: Iterator<Item = &'a u8>>(iter: &mut I, end: u8) -> Result<String, Error> {
    let mut v = Vec::new();
    loop {
        let c = try_next!(iter).clone();
        if c == end {
            break;
        }
        v.push(c);
    }
    Ok(String::from_utf8_lossy(&v).into_owned())
}

pub fn hex_str(v: &u8) -> String {
    match v / 16 == 0 {
        true => format!("0{:x}", v),
        false => format!("{:x}", v),
    }
}
