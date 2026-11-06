//! Bounded input independent of GTK; workers can use local files or portal streams.
use std::io::{self, Read};
pub fn read_limited(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Maximum backup size is 40 MB.",
        ));
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_limit_rejects_excess_and_bounds_reads() {
        assert_eq!(read_limited(&b"abc"[..], 3).unwrap(), b"abc");
        let mut input = &b"abcdef"[..];
        assert!(read_limited(&mut input, 3).is_err());
        assert_eq!(input, b"ef");
    }
    #[test]
    fn propagates_read_errors() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        assert_eq!(
            read_limited(Broken, 3).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
