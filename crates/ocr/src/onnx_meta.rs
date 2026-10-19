//! `metadata_props` of an ONNX `ModelProto`, read straight off the protobuf
//! wire format: rten loads the graph but does not expose these entries, and
//! the recognition model keeps its alphabet in one of them

/// Value stored under `key` in the model's `metadata_props`.
pub(crate) fn metadata_value(model: &[u8], key: &str) -> Option<String> {
    let mut r = Reader { buf: model, pos: 0 };
    while let Some((field, wire)) = r.tag() {
        match (field, wire) {
            (14, 2) => {
                let entry = r.bytes()?;
                if let Some(v) = entry_value(entry, key) {
                    return Some(v);
                }
            }
            _ => r.skip(wire)?,
        }
    }
    None
}

fn entry_value(entry: &[u8], key: &str) -> Option<String> {
    let mut r = Reader { buf: entry, pos: 0 };
    let mut k = None;
    let mut v = None;
    while let Some((field, wire)) = r.tag() {
        match (field, wire) {
            (1, 2) => k = Some(r.bytes()?),
            (2, 2) => v = Some(r.bytes()?),
            _ => r.skip(wire)?,
        }
    }
    if k? != key.as_bytes() {
        return None;
    }
    String::from_utf8(v?.to_vec()).ok()
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.buf.get(self.pos)?;
            self.pos += 1;
            out |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(out);
            }
        }
        None
    }

    fn tag(&mut self) -> Option<(u64, u8)> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let t = self.varint()?;
        Some((t >> 3, (t & 7) as u8))
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.varint()?).ok()?;
        let end = self.pos.checked_add(len)?;
        let out = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => self.varint().map(|_| ()),
            1 => self.advance(8),
            2 => self.bytes().map(|_| ()),
            5 => self.advance(4),
            _ => None,
        }
    }

    fn advance(&mut self, n: usize) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        (end <= self.buf.len()).then(|| self.pos = end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(n: u64, body: &[u8]) -> Vec<u8> {
        let mut out = vec![(n << 3 | 2) as u8];
        out.push(body.len() as u8);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn finds_the_value_behind_other_fields() {
        let mut model = vec![0x08, 0x07]; // ir_version = 7
        model.extend(field(2, b"paddle"));
        model.extend(field(14, &[field(1, b"other"), field(2, b"x")].concat()));
        model.extend(field(
            14,
            &[field(1, b"character"), field(2, "あ\nb".as_bytes())].concat(),
        ));
        assert_eq!(
            metadata_value(&model, "character").as_deref(),
            Some("あ\nb")
        );
        assert_eq!(metadata_value(&model, "missing"), None);
    }

    #[test]
    fn a_truncated_model_reads_as_absent() {
        let model = field(14, &[field(1, b"character"), field(2, b"abc")].concat());
        assert_eq!(metadata_value(&model[..model.len() - 2], "character"), None);
    }
}
