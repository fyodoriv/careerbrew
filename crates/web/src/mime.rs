//! `mimetypes.guess_type(path, strict=True)[0]` as Python 3.12 answers it:
//! the built-in strict table, then the system `mime.types` files in Python's
//! `knownfiles` order (a later file wins), then the suffix and encoding maps.

use std::collections::HashMap;
use std::sync::OnceLock;

#[rustfmt::skip]
const BUILTIN: &[(&str, &str)] = &[
    (".3g2", "audio/3gpp2"), (".3gp", "audio/3gpp"), (".3gpp", "audio/3gpp"), (".3gpp2", "audio/3gpp2"),
    (".a", "application/octet-stream"), (".aac", "audio/aac"), (".adts", "audio/aac"), (".ai", "application/postscript"),
    (".aif", "audio/x-aiff"), (".aifc", "audio/x-aiff"), (".aiff", "audio/x-aiff"), (".ass", "audio/aac"),
    (".au", "audio/basic"), (".avi", "video/x-msvideo"), (".avif", "image/avif"), (".bat", "text/plain"),
    (".bcpio", "application/x-bcpio"), (".bin", "application/octet-stream"), (".bmp", "image/bmp"), (".c", "text/plain"),
    (".cdf", "application/x-netcdf"), (".cpio", "application/x-cpio"), (".csh", "application/x-csh"), (".css", "text/css"),
    (".csv", "text/csv"), (".dll", "application/octet-stream"), (".doc", "application/msword"), (".dot", "application/msword"),
    (".dvi", "application/x-dvi"), (".eml", "message/rfc822"), (".eps", "application/postscript"), (".etx", "text/x-setext"),
    (".exe", "application/octet-stream"), (".gif", "image/gif"), (".gtar", "application/x-gtar"), (".h", "text/plain"),
    (".h5", "application/x-hdf5"), (".hdf", "application/x-hdf"), (".heic", "image/heic"), (".heif", "image/heif"),
    (".htm", "text/html"), (".html", "text/html"), (".ico", "image/vnd.microsoft.icon"), (".ief", "image/ief"),
    (".jpe", "image/jpeg"), (".jpeg", "image/jpeg"), (".jpg", "image/jpeg"), (".js", "text/javascript"),
    (".json", "application/json"), (".ksh", "text/plain"), (".latex", "application/x-latex"), (".loas", "audio/aac"),
    (".m1v", "video/mpeg"), (".m3u", "application/vnd.apple.mpegurl"), (".m3u8", "application/vnd.apple.mpegurl"), (".man", "application/x-troff-man"),
    (".markdown", "text/markdown"), (".md", "text/markdown"), (".me", "application/x-troff-me"), (".mht", "message/rfc822"),
    (".mhtml", "message/rfc822"), (".mif", "application/x-mif"), (".mjs", "text/javascript"), (".mov", "video/quicktime"),
    (".movie", "video/x-sgi-movie"), (".mp2", "audio/mpeg"), (".mp3", "audio/mpeg"), (".mp4", "video/mp4"),
    (".mpa", "video/mpeg"), (".mpe", "video/mpeg"), (".mpeg", "video/mpeg"), (".mpg", "video/mpeg"),
    (".ms", "application/x-troff-ms"), (".n3", "text/n3"), (".nc", "application/x-netcdf"), (".nq", "application/n-quads"),
    (".nt", "application/n-triples"), (".nws", "message/rfc822"), (".o", "application/octet-stream"), (".obj", "application/octet-stream"),
    (".oda", "application/oda"), (".opus", "audio/opus"), (".p12", "application/x-pkcs12"), (".p7c", "application/pkcs7-mime"),
    (".pbm", "image/x-portable-bitmap"), (".pdf", "application/pdf"), (".pfx", "application/x-pkcs12"), (".pgm", "image/x-portable-graymap"),
    (".pl", "text/plain"), (".png", "image/png"), (".pnm", "image/x-portable-anymap"), (".pot", "application/vnd.ms-powerpoint"),
    (".ppa", "application/vnd.ms-powerpoint"), (".ppm", "image/x-portable-pixmap"), (".pps", "application/vnd.ms-powerpoint"), (".ppt", "application/vnd.ms-powerpoint"),
    (".ps", "application/postscript"), (".pwz", "application/vnd.ms-powerpoint"), (".py", "text/x-python"), (".pyc", "application/x-python-code"),
    (".pyo", "application/x-python-code"), (".qt", "video/quicktime"), (".ra", "audio/x-pn-realaudio"), (".ram", "application/x-pn-realaudio"),
    (".ras", "image/x-cmu-raster"), (".rdf", "application/xml"), (".rgb", "image/x-rgb"), (".roff", "application/x-troff"),
    (".rst", "text/x-rst"), (".rtx", "text/richtext"), (".sgm", "text/x-sgml"), (".sgml", "text/x-sgml"),
    (".sh", "application/x-sh"), (".shar", "application/x-shar"), (".snd", "audio/basic"), (".so", "application/octet-stream"),
    (".src", "application/x-wais-source"), (".srt", "text/plain"), (".sv4cpio", "application/x-sv4cpio"), (".sv4crc", "application/x-sv4crc"),
    (".svg", "image/svg+xml"), (".swf", "application/x-shockwave-flash"), (".t", "application/x-troff"), (".tar", "application/x-tar"),
    (".tcl", "application/x-tcl"), (".tex", "application/x-tex"), (".texi", "application/x-texinfo"), (".texinfo", "application/x-texinfo"),
    (".tif", "image/tiff"), (".tiff", "image/tiff"), (".tr", "application/x-troff"), (".trig", "application/trig"),
    (".tsv", "text/tab-separated-values"), (".txt", "text/plain"), (".ustar", "application/x-ustar"), (".vcf", "text/x-vcard"),
    (".vtt", "text/vtt"), (".wasm", "application/wasm"), (".wav", "audio/x-wav"), (".webm", "video/webm"),
    (".webmanifest", "application/manifest+json"), (".wiz", "application/msword"), (".wsdl", "application/xml"), (".xbm", "image/x-xbitmap"),
    (".xlb", "application/vnd.ms-excel"), (".xls", "application/vnd.ms-excel"), (".xml", "text/xml"), (".xpdl", "application/xml"),
    (".xpm", "image/x-xpixmap"), (".xsl", "application/xml"), (".xwd", "image/x-xwindowdump"), (".zip", "application/zip"),
];

#[rustfmt::skip]
const SUFFIX: &[(&str, &str)] = &[
    (".svgz", ".svg.gz"), (".tgz", ".tar.gz"), (".taz", ".tar.gz"), (".tz", ".tar.gz"), (".tbz2", ".tar.bz2"), (".txz", ".tar.xz"),
];
const ENCODINGS: &[&str] = &[".gz", ".Z", ".bz2", ".xz", ".br"];

/// Python's `mimetypes.knownfiles`, read in this order.
pub const KNOWN_FILES: &[&str] = &[
    "/etc/mime.types",
    "/etc/httpd/mime.types",
    "/etc/httpd/conf/mime.types",
    "/etc/apache/mime.types",
    "/etc/apache2/mime.types",
    "/usr/local/etc/httpd/conf/mime.types",
    "/usr/local/lib/netscape/mime.types",
    "/usr/local/etc/httpd/conf/mime.types",
    "/usr/local/etc/mime.types",
];

pub fn table(files: &[&str]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = BUILTIN
        .iter()
        .map(|(e, t)| ((*e).to_owned(), (*t).to_owned()))
        .collect();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            let mut words = Vec::new();
            for word in line.split_whitespace() {
                if word.starts_with('#') {
                    break;
                }
                words.push(word);
            }
            if let Some((kind, suffixes)) = words.split_first() {
                for suffix in suffixes {
                    map.insert(format!(".{suffix}"), (*kind).to_owned());
                }
            }
        }
    }
    map
}

fn system() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| table(KNOWN_FILES))
}

fn splitext(path: &str) -> (&str, &str) {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    let name = &path[name_start..];
    let leading = name.len() - name.trim_start_matches('.').len();
    match name[leading..].rfind('.') {
        Some(dot) => path.split_at(name_start + leading + dot),
        None => (path, ""),
    }
}

pub fn guess_with(map: &HashMap<String, String>, path: &str) -> Option<String> {
    let mut owned = path.to_owned();
    let (mut base, mut ext) = {
        let (b, e) = splitext(&owned);
        (b.to_owned(), e.to_owned())
    };
    while let Some((_, to)) = SUFFIX.iter().find(|(from, _)| *from == ext.to_lowercase()) {
        owned = format!("{base}{to}");
        let (b, e) = splitext(&owned);
        (base, ext) = (b.to_owned(), e.to_owned());
    }
    if ENCODINGS.contains(&ext.as_str()) {
        let (_, e) = splitext(&base);
        ext = e.to_owned();
    }
    map.get(&ext.to_lowercase()).cloned()
}

pub fn guess(path: &str) -> Option<String> {
    guess_with(system(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_and_file_order() {
        let map = table(&[]);
        assert_eq!(
            guess_with(&map, "/x/app.JS").as_deref(),
            Some("text/javascript")
        );
        assert_eq!(
            guess_with(&map, "/x/a.svgz").as_deref(),
            Some("image/svg+xml")
        );
        assert_eq!(
            guess_with(&map, "/x/a.js.gz").as_deref(),
            Some("text/javascript")
        );
        assert_eq!(guess_with(&map, "/x/.hidden"), None);
        assert_eq!(guess_with(&map, "/x/a.map"), None);
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("mime.types");
        std::fs::write(&file, "# c\napplication/javascript js mjs # x\n").unwrap();
        let path = file.to_string_lossy().into_owned();
        let map = table(&[&path]);
        assert_eq!(
            guess_with(&map, "/x/app.js").as_deref(),
            Some("application/javascript")
        );
    }
}
