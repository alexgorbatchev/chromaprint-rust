use crate::Error;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusty_chromaprint::{Configuration, FingerprintCompressor};

/// Compress raw fingerprints using Chromaprint's native three/five-bit format.
pub fn encode(raw: &[u32], algorithm: u8) -> Result<Vec<u8>, Error> {
    if raw.len() > 0xffffff {
        return Err(Error::FingerprintTooLong);
    }
    let config = Configuration::default().with_id(algorithm);
    Ok(FingerprintCompressor::from(&config).compress(raw))
}

pub fn encode_base64(raw: &[u32], algorithm: u8) -> Result<String, Error> {
    Ok(URL_SAFE_NO_PAD.encode(encode(raw, algorithm)?))
}

pub fn decode_header(input: &[u8]) -> Result<(usize, u8), Error> {
    if input.len() < 4 {
        return Err(Error::InvalidFingerprint);
    }
    Ok((
        ((input[1] as usize) << 16) | ((input[2] as usize) << 8) | input[3] as usize,
        input[0],
    ))
}

pub fn decode_header_base64(input: &str) -> Result<(usize, u8), Error> {
    decode_header(&URL_SAFE_NO_PAD.decode(&input.as_bytes()[..input.len().min(8)])?)
}

fn unpack(input: &[u8], width: usize, index: usize) -> Option<u8> {
    let bit = index * width;
    if bit + width > input.len() * 8 {
        return None;
    }
    let mut value = 0;
    for shift in 0..width {
        value |= ((input[(bit + shift) / 8] >> ((bit + shift) % 8)) & 1) << shift;
    }
    Some(value)
}

pub fn decode(input: &[u8]) -> Result<(Vec<u32>, u8), Error> {
    let (size, algorithm) = decode_header(input)?;
    // A normal stream contains at least one three-bit terminator per item.
    // Bound allocation using the actual payload before trusting the header.
    if size > input[4..].len() * 8 / 3 {
        return Err(Error::InvalidFingerprint);
    }
    let mut normals = Vec::new();
    let mut ended = 0;
    let mut index = 0;
    while ended < size {
        let value = unpack(&input[4..], 3, index).ok_or(Error::InvalidFingerprint)?;
        normals.push(value);
        if value == 0 {
            ended += 1;
        }
        index += 1;
    }
    let offset = 4 + (normals.len() * 3).div_ceil(8);
    let exceptions = &input[offset..];
    let mut exceptional_index = 0;
    let mut raw = Vec::with_capacity(size);
    let mut value = 0u32;
    let mut bit = 0usize;
    for mut normal in normals {
        if normal == 7 {
            normal += unpack(exceptions, 5, exceptional_index).ok_or(Error::InvalidFingerprint)?;
            exceptional_index += 1;
        }
        if normal == 0 {
            raw.push(value);
            bit = 0;
        } else {
            bit += normal as usize;
            if bit > 32 {
                return Err(Error::InvalidFingerprint);
            }
            value ^= 1 << (bit - 1);
        }
    }
    Ok((raw, algorithm))
}

pub fn decode_base64(input: &str) -> Result<(Vec<u32>, u8), Error> {
    decode(&URL_SAFE_NO_PAD.decode(input)?)
}
