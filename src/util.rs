extern crate std;
extern crate byteorder;

use errors;

use std::io::Cursor;
use self::byteorder::{BigEndian, ReadBytesExt};
use errors::Error;

pub fn next_item<T, IT: Iterator<Item = T>>(iter: &mut IT) -> errors::Result<T> {
    iter.next().ok_or(
        errors::ErrorKind::InvalidPacketError("Early EOF while parsing packet".into()).into(),
    )
}

pub fn next_items<T, IT: Iterator<Item = T>>(
    iter: &mut IT,
    n: usize,
) -> errors::Result<Vec<T>> {
    let mut v = Vec::new();
    for _ in 0..n {
        v.push(next_item(iter)?)
    }
    Ok(v)
}

/*
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
*/

pub fn to_u16_dyn(slice: &[u8]) -> errors::Result<u16> {
    Ok(Cursor::new(slice).read_u16::<BigEndian>()?)
}

pub fn to_u16(slice: &[u8; 2]) -> u16 {
    to_u16_dyn(slice).unwrap()
}

pub fn to_u32_dyn(slice: &[u8]) -> errors::Result<u32> {
    Ok(Cursor::new(slice).read_u32::<BigEndian>()?)
}

pub fn to_u32(slice: &[u8; 4]) -> u32 {
    to_u32_dyn(slice).unwrap()
}

pub fn read_string<IT: Iterator<Item = u8>>(iter: &mut IT, end: u8) -> Result<String, Error> {
    let mut v = Vec::new();
    loop {
        let c = next_item(iter)?.clone();
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

pub trait LoggingService {
    fn debug(&self, msg: &str);
    fn info(&self, msg: &str);
}

#[derive(Clone)]
pub struct RealLogger;

impl LoggingService for RealLogger {
    fn debug(&self, msg: &str) {
        println!("DEBUG: {}", msg)
    }

    fn info(&self, msg: &str) {
        println!("INFO: {}", msg)
    }
}
