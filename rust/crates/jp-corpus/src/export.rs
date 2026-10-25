//! 两端共用的文件写入校验。
pub fn text(path: String, content: String) -> anyhow::Result<()> {
    let target = std::path::Path::new(&path);
    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "txt" | "csv" | "html") {
        return Err(anyhow::anyhow!("只能导出 .txt / .csv / .html：{path}"));
    }
    std::fs::write(target, content).map_err(|err| anyhow::anyhow!("写不了 {path}：{err}"))?;
    Ok(())
}
pub fn png(path: String, bytes: Vec<u8>) -> anyhow::Result<()> {
    let target = std::path::Path::new(&path);
    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "png" {
        return Err(anyhow::anyhow!("只能导出 .png：{path}"));
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(anyhow::anyhow!("不是 PNG 数据，没有写入"));
    }
    std::fs::write(target, bytes).map_err(|err| anyhow::anyhow!("写不了 {path}：{err}"))?;
    Ok(())
}
