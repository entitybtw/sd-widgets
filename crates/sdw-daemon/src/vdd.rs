//! Virtual Display Driver on Windows: where its settings live and how its XML
//! is edited.
//!
//! https://github.com/VirtualDrivers/Virtual-Display-Driver
//!
//! The driver reads `vdd_settings.xml` when its adapter starts, so a new
//! resolution is added to the file and then the adapter is reloaded through the
//! driver's own control pipe. Everything here is plain text handling and can be
//! tested on any platform; the Windows side lives in `win::display`.

use std::path::{Path, PathBuf};

/// Where the driver keeps its settings unless the installer was told otherwise.
pub const DEFAULT_SETTINGS: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";

/// The settings file, or `None` when the driver is not installed.
///
/// `SDW_VDD_SETTINGS` overrides the location for a custom install (and lets the
/// XML editor be exercised without a Windows machine).
pub fn settings_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SDW_VDD_SETTINGS") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path = PathBuf::from(DEFAULT_SETTINGS);
    path.is_file().then_some(path)
}

/// The path to show in the window and in errors, whether or not it exists.
pub fn settings_path_display() -> String {
    settings_path()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SETTINGS))
        .display()
        .to_string()
}

/// A complete, conservative settings file. Used when the driver is installed
/// but its file is missing, so the first start does not need the vendor app.
pub fn minimal(width: u32, height: u32, refresh: u32) -> String {
    format!(
        r#"<?xml version='1.0' encoding='utf-8'?>
<vdd_settings>
    <monitors>
        <count>1</count>
    </monitors>
    <gpu>
        <friendlyname>default</friendlyname>
    </gpu>
    <global>
        <g_refresh_rate>60</g_refresh_rate>
        <g_refresh_rate>90</g_refresh_rate>
        <g_refresh_rate>120</g_refresh_rate>
    </global>
    <resolutions>
        <resolution>
            <width>1920</width>
            <height>1080</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
        <resolution>
            <width>{width}</width>
            <height>{height}</height>
            <refresh_rate>{refresh}</refresh_rate>
        </resolution>
    </resolutions>
    <logging>
        <SendLogsThroughPipe>true</SendLogsThroughPipe>
        <logging>false</logging>
        <debuglogging>false</debuglogging>
    </logging>
    <colour>
        <SDR10bit>false</SDR10bit>
        <HDRPlus>false</HDRPlus>
        <ColourFormat>RGB</ColourFormat>
    </colour>
    <cursor>
        <HardwareCursor>true</HardwareCursor>
    </cursor>
    <edid>
        <CustomEdid>false</CustomEdid>
        <PreventSpoof>false</PreventSpoof>
        <EdidCeaOverride>false</EdidCeaOverride>
    </edid>
</vdd_settings>
"#
    )
}

/// Number of virtual monitors the driver is told to create.
pub fn monitor_count(xml: &str) -> Option<u32> {
    let inner = inner_of(xml, "count")?;
    inner.trim().parse().ok()
}

/// Set `<monitors><count>`, adding the section when an older file has none.
pub fn set_count(xml: &str, count: u32) -> Result<String, String> {
    let count = count.clamp(1, 8);
    if let Some((start, end)) = inner_span(xml, "count") {
        let mut out = String::with_capacity(xml.len() + 4);
        out.push_str(&xml[..start]);
        out.push_str(&count.to_string());
        out.push_str(&xml[end..]);
        return Ok(out);
    }
    let section = format!("<monitors><count>{count}</count></monitors>");
    if let Some((start, end)) = block_span(xml, "monitors") {
        let mut out = String::with_capacity(xml.len() + 32);
        out.push_str(&xml[..start]);
        out.push_str(&format!("<count>{count}</count>"));
        out.push_str(&xml[start..end]);
        out.push_str(&xml[end..]);
        return Ok(out);
    }
    insert_before_close(xml, &section)
}

/// True when the file already advertises this width and height, at any refresh
/// rate.
pub fn has_resolution(xml: &str, width: u32, height: u32) -> bool {
    let Some((start, end)) = block_span(xml, "resolutions") else {
        return false;
    };
    let block = &xml[start..end];
    let (w, h) = (width.to_string(), height.to_string());
    let mut rest = block;
    while let Some((s, e)) = block_span(rest, "resolution") {
        let one = &rest[s..e];
        if inner_of(one, "width")
            .map(|v| v.trim() == w)
            .unwrap_or(false)
            && inner_of(one, "height")
                .map(|v| v.trim() == h)
                .unwrap_or(false)
        {
            return true;
        }
        rest = &rest[e..];
    }
    false
}

/// Add `width`×`height` at `refresh` to `<resolutions>` and the refresh rate to
/// `<global>`, leaving every other setting of the file alone.
pub fn ensure_mode(xml: &str, width: u32, height: u32, refresh: u32) -> Result<String, String> {
    if !xml.contains("</vdd_settings>") {
        return Err("vdd_settings.xml повреждён: нет закрывающего тега".into());
    }
    let width = width.clamp(320, 7680);
    let height = height.clamp(240, 4320);
    let refresh = refresh.clamp(24, 500);
    let mut out = xml.to_string();

    if !has_resolution(&out, width, height) {
        let resolution = format!(
            "<resolution><width>{width}</width><height>{height}</height>\
             <refresh_rate>{refresh}</refresh_rate></resolution>"
        );
        out = if block_span(&out, "resolutions").is_some() {
            insert_inside(&out, "resolutions", &resolution)?
        } else {
            insert_before_close(&out, &format!("<resolutions>{resolution}</resolutions>"))?
        };
    }

    let rate = format!("<g_refresh_rate>{refresh}</g_refresh_rate>");
    if inner_spans(&out, "g_refresh_rate")
        .iter()
        .all(|(s, e)| out[*s..*e].trim() != refresh.to_string())
    {
        out = if block_span(&out, "global").is_some() {
            insert_inside(&out, "global", &rate)?
        } else {
            insert_before_close(&out, &format!("<global>{rate}</global>"))?
        };
    }
    Ok(out)
}

// ---------------------------------------------------------------- xml helpers

/// The text between `<tag>` and `</tag>`, first occurrence.
fn inner_of<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let (start, end) = inner_span(xml, tag)?;
    Some(&xml[start..end])
}

/// Byte span of the text between `<tag>` and its `</tag>`.
fn inner_span(xml: &str, tag: &str) -> Option<(usize, usize)> {
    inner_spans(xml, tag).into_iter().next()
}

fn inner_spans<'a>(xml: &'a str, tag: &str) -> Vec<(usize, usize)> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(offset) = xml[from..].find(&open) {
        let start = from + offset + open.len();
        let Some(end) = xml[start..].find(&close).map(|e| start + e) else {
            break;
        };
        spans.push((start, end));
        from = end + close.len();
    }
    spans
}

/// Byte span of a whole `<tag>…</tag>` block, including the tags.
fn block_span(xml: &str, tag: &str) -> Option<(usize, usize)> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let end = xml[start..].find(&close)? + start + close.len();
    Some((start, end))
}

/// Put `text` just before the closing tag of `tag`.
fn insert_inside(xml: &str, tag: &str, text: &str) -> Result<String, String> {
    let close = format!("</{tag}>");
    let Some(pos) = xml.find(&close) else {
        return Err(format!("в vdd_settings.xml нет </{tag}>"));
    };
    let mut out = String::with_capacity(xml.len() + text.len());
    out.push_str(&xml[..pos]);
    out.push_str(text);
    out.push_str(&xml[pos..]);
    Ok(out)
}

fn insert_before_close(xml: &str, text: &str) -> Result<String, String> {
    insert_inside(xml, "vdd_settings", text)
}

/// `true` when `path` names the file [`settings_path`] would return.
pub fn is_settings_path(path: &Path) -> bool {
    settings_path().map(|p| p == path).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<vdd_settings>
    <monitors>
        <count>1</count>
    </monitors>
    <gpu><friendlyname>default</friendlyname></gpu>
    <global>
        <g_refresh_rate>60</g_refresh_rate>
    </global>
    <resolutions>
        <resolution>
            <width>1920</width>
            <height>1080</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
    </resolutions>
</vdd_settings>
"#;

    #[test]
    fn count_is_replaced_in_place() {
        assert_eq!(
            set_count(SAMPLE, 2).unwrap().contains("<count>2</count>"),
            true
        );
        assert_eq!(monitor_count(SAMPLE), Some(1));
        assert_eq!(monitor_count(&set_count(SAMPLE, 3).unwrap()), Some(3));
    }

    #[test]
    fn a_missing_resolution_is_added_once() {
        let once = ensure_mode(SAMPLE, 1280, 800, 60).unwrap();
        assert!(has_resolution(&once, 1280, 800));
        let twice = ensure_mode(&once, 1280, 800, 60).unwrap();
        assert_eq!(once, twice);
        assert_eq!(twice.matches("<width>1280</width>").count(), 1);
        assert_eq!(
            twice.matches("<g_refresh_rate>60</g_refresh_rate>").count(),
            1
        );
    }

    #[test]
    fn an_old_file_without_sections_is_still_edited() {
        let old = "<?xml version='1.0'?>\n<vdd_settings>\n</vdd_settings>\n";
        let out = ensure_mode(old, 1280, 800, 60).unwrap();
        assert!(out.contains("<resolutions><resolution><width>1280</width>"));
        assert!(out.contains("<global><g_refresh_rate>60</g_refresh_rate></global>"));
        let out = set_count(&out, 1).unwrap();
        assert_eq!(monitor_count(&out), Some(1));
    }

    #[test]
    fn a_broken_file_is_refused() {
        assert!(ensure_mode("<vdd_settings>", 1280, 800, 60).is_err());
        assert!(set_count("<vdd_settings>", 1).is_err());
    }

    #[test]
    fn minimal_file_carries_the_requested_mode() {
        let xml = minimal(1280, 800, 60);
        assert!(has_resolution(&xml, 1280, 800));
        assert_eq!(monitor_count(&xml), Some(1));
        assert!(ensure_mode(&xml, 1280, 800, 60).unwrap() == xml);
    }
}
