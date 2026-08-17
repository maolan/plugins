#[derive(Debug)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_fourcc(&mut self) -> Result<[u8; 4], String> {
        if self.remaining() < 4 {
            return Err("Unexpected EOF reading fourcc".to_string());
        }
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(buf)
    }

    pub fn read_u8(&mut self) -> Result<u8, String> {
        if self.remaining() < 1 {
            return Err("Unexpected EOF".to_string());
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_i8(&mut self) -> Result<i8, String> {
        self.read_u8().map(|v| v as i8)
    }

    pub fn read_u16(&mut self) -> Result<u16, String> {
        if self.remaining() < 2 {
            return Err("Unexpected EOF".to_string());
        }
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_i16(&mut self) -> Result<i16, String> {
        self.read_u16().map(|v| v as i16)
    }

    pub fn read_u32(&mut self) -> Result<u32, String> {
        if self.remaining() < 4 {
            return Err("Unexpected EOF".to_string());
        }
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.remaining() < len {
            return Err("Unexpected EOF".to_string());
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    pub fn skip(&mut self, len: usize) -> Result<(), String> {
        if self.remaining() < len {
            return Err("Unexpected EOF".to_string());
        }
        self.pos += len;
        Ok(())
    }

    pub fn read_string(&mut self, len: usize) -> Result<String, String> {
        let bytes = self.read_bytes(len)?;
        let mut s = String::from_utf8_lossy(bytes).into_owned();
        if let Some(nul) = s.find('\0') {
            s.truncate(nul);
        }
        Ok(s)
    }
}

pub fn fourcc_str(fcc: [u8; 4]) -> String {
    String::from_utf8_lossy(&fcc).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_reader() {
        let data = b"RIFF\x10\x00\x00\x00sfbk";
        let mut r = ByteReader::new(data);
        assert_eq!(r.read_fourcc().unwrap(), *b"RIFF");
        assert_eq!(r.read_u32().unwrap(), 16);
        assert_eq!(r.read_fourcc().unwrap(), *b"sfbk");
    }
}
