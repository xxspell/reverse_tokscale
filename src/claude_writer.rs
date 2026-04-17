use std::io::Write;

use anyhow::Result;

pub fn append_jsonl_line(mut writer: impl Write, line: &str) -> Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    Ok(())
}
