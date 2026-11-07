use std::{env, fs, path::Path, process, str, thread, time::Duration, time::SystemTime};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};

const INTERVAL: Duration = Duration::from_millis(250);

fn main() {
    let Some(dir) = env::args().nth(1) else {
        eprintln!("usage: rippie-mac <shared-dir>");
        process::exit(2);
    };
    let path = Path::new(&dir).join(".rippie").join("clipboard");

    // Start from the current file state so a stale buffer doesn't clobber the clipboard.
    let mut last = stamp(&path);
    loop {
        thread::sleep(INTERVAL);
        let now = stamp(&path);
        if now.is_none() || now == last {
            continue;
        }
        last = now;

        let buf = match fs::read(&path) {
            Ok(buf) => buf,
            Err(e) => {
                eprintln!("rippie-mac: read {}: {e}", path.display());
                continue;
            }
        };
        match parse(&buf) {
            Some(items) => autoreleasepool(|_| set_pasteboard(&items)),
            None => eprintln!("rippie-mac: malformed {}", path.display()),
        }
    }
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Parses records of `<mime>\n<len>\n<bytes>` into (UTI, bytes), dropping unknown mime types.
fn parse(mut buf: &[u8]) -> Option<Vec<(&'static str, &[u8])>> {
    let mut items = Vec::new();
    while !buf.is_empty() {
        let (mime, rest) = split_line(buf)?;
        let (len, rest) = split_line(rest)?;
        let len: usize = str::from_utf8(len).ok()?.parse().ok()?;
        if rest.len() < len {
            return None;
        }
        let (data, rest) = rest.split_at(len);
        if let Some(uti) = str::from_utf8(mime).ok().and_then(uti) {
            items.push((uti, data));
        }
        buf = rest;
    }
    Some(items)
}

fn split_line(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let i = buf.iter().position(|&b| b == b'\n')?;
    Some((&buf[..i], &buf[i + 1..]))
}

fn uti(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "text/plain;charset=utf-8" | "UTF8_STRING" | "text/plain" => "public.utf8-plain-text",
        "text/html" => "public.html",
        "text/rtf" | "application/rtf" => "public.rtf",
        "image/png" => "public.png",
        "image/tiff" => "public.tiff",
        "image/jpeg" => "public.jpeg",
        "image/gif" => "com.compuserve.gif",
        _ => return None,
    })
}

fn set_pasteboard(items: &[(&str, &[u8])]) {
    let pb = NSPasteboard::generalPasteboard();
    let items: Vec<_> = items
        .iter()
        .map(|(uti, data)| (NSString::from_str(uti), *data))
        .collect();
    // Content that just round-tripped from the mac is already there; don't strip its other formats.
    if items
        .iter()
        .all(|(ty, data)| pb.dataForType(ty).is_some_and(|d| d.to_vec() == *data))
    {
        return;
    }
    pb.clearContents();
    for (ty, data) in &items {
        pb.setData_forType(Some(&NSData::with_bytes(data)), ty);
    }
}
