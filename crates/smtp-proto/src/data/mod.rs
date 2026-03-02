/// remove dot-stuffing from a single SMTP DATA line.
/// returns None if the line is the terminator ".\r\n"
pub fn unstuff_line(line: &[u8]) -> Option<&[u8]> {
    if line == b".\r\n" {
        return None;
    }
    if line.starts_with(b"..") {
        Some(&line[1..])
    } else {
        Some(line)
    }
}

/// process a complete DATA payload (lines terminated by CRLF, ending with ".\r\n").
/// removes dot-stuffing and returns the message body without the terminator.
pub fn unstuff_data(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut start = 0;
    while start < data.len() {
        let end = match data[start..].iter().position(|&b| b == b'\n') {
            Some(pos) => start + pos + 1,
            None => data.len(),
        };
        let line = &data[start..end];
        match unstuff_line(line) {
            Some(unstuffed) => result.extend_from_slice(unstuffed),
            None => break,
        }
        start = end;
    }
    result
}

#[cfg(test)]
mod tests;
