use std::path::Path;

use crate::mime_type::MimeTypeError;

pub trait Clipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
    fn write_image(&self, path: &Path) -> Result<(), ClipboardError>;
}

/// The clipboard for this platform: AppKit's on macOS, Wayland's on Linux.
pub fn clipboard() -> Result<Box<dyn Clipboard>, ClipboardError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacClipboard::general()))
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Ok(Box::new(wayland::WaylandClipboard))
        } else {
            Err(ClipboardError::UnsupportedPlatform)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(ClipboardError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "linux")]
mod wayland {
    use std::{
        io::Write,
        path::Path,
        process::{Command, Stdio},
    };

    use super::{Clipboard, ClipboardError};

    pub(super) struct WaylandClipboard;

    impl Clipboard for WaylandClipboard {
        fn write(&self, text: &str) -> Result<(), ClipboardError> {
            let mut child = Command::new("wl-copy").stdin(Stdio::piped()).spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes())?;
            }

            Ok(())
        }

        fn write_image(&self, path: &Path) -> Result<(), ClipboardError> {
            let uri = format!("file://{}\n", percent_encode_path(path));

            let mut child = Command::new("wl-copy").arg("--type").arg("text/uri-list").stdin(Stdio::piped()).spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(uri.as_bytes())?;
            }

            Ok(())
        }
    }

    /// A path as a file URI path: bytes outside RFC 3986's unreserved set
    /// (and `/`) become `%XX`, so spaces and `%` survive the uri-list.
    fn percent_encode_path(path: &Path) -> String {
        use std::os::unix::ffi::OsStrExt;
        let mut out = String::new();
        for &byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn percent_encodes_paths() {
            let path = std::path::Path::new("/home/doug/Pictures/kura/cat jam%1.gif");
            assert_eq!(super::percent_encode_path(path), "/home/doug/Pictures/kura/cat%20jam%251.gif");
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        io::{Read, Write},
        path::Path,
        process::{Command, Stdio},
    };

    use super::{Clipboard, ClipboardError};
    use crate::mime_type::MimeType;

    /// Writes to an NSPasteboard through AppKit (AppleScriptObjC, so no extra
    /// crates). Arguments: pasteboard name ("" for the general one), then
    /// either `text <string>` or `image <path> <type identifier>`. An image goes
    /// on as its data and as a file URL: apps that paste pictures take the data;
    /// Messages, Slack and Discord attach the file, which keeps GIFs animated.
    const WRITE: &str = r#"use framework "AppKit"
on run argv
    set ca to current application
    set theItem to ca's NSPasteboardItem's new()
    if item 2 of argv is "text" then
        theItem's setString:(item 3 of argv) forType:"public.utf8-plain-text"
    else
        set {thePath, theType} to items 3 thru 4 of argv
        set theData to ca's NSData's dataWithContentsOfFile:thePath
        if theData is missing value then error "can't read " & thePath
        theItem's setData:theData forType:theType
        theItem's setString:((ca's NSURL's fileURLWithPath:thePath)'s absoluteString()) forType:"public.file-url"
    end if
    if item 1 of argv is "" then
        set board to ca's NSPasteboard's generalPasteboard()
    else
        set board to ca's NSPasteboard's pasteboardWithName:(item 1 of argv)
    end if
    board's clearContents()
    if not (board's writeObjects:{theItem}) then error "the pasteboard refused the item"
end run"#;

    pub(super) struct MacClipboard {
        /// "" for the general (system) clipboard; tests use private, named ones.
        pasteboard: String,
    }

    impl MacClipboard {
        pub(super) fn general() -> Self {
            Self { pasteboard: String::new() }
        }

        #[cfg(test)]
        fn named(name: &str) -> Self {
            Self { pasteboard: name.to_owned() }
        }

        fn run(&self, args: &[&str]) -> Result<(), ClipboardError> {
            let mut child = Command::new("osascript")
                .arg("-")
                .arg(&self.pasteboard)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            child.stdin.take().expect("stdin is piped").write_all(WRITE.as_bytes())?;
            let output = child.wait_with_output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(ClipboardError::Command(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
            }
        }
    }

    impl Clipboard for MacClipboard {
        fn write(&self, text: &str) -> Result<(), ClipboardError> {
            self.run(&["text", text])
        }

        fn write_image(&self, path: &Path) -> Result<(), ClipboardError> {
            let path = path.canonicalize()?;
            let mut head = [0; 12];
            let read = std::fs::File::open(&path)?.read(&mut head)?;
            let type_id = match MimeType::from_bytes(&head[..read])? {
                MimeType::Png => "public.png",
                MimeType::Jpeg => "public.jpeg",
                MimeType::Gif => "com.compuserve.gif",
                MimeType::WebP => "org.webmproject.webp",
            };
            let path = path.to_str().ok_or(ClipboardError::NonUtf8Path)?;
            self.run(&["image", path, type_id])
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Reads a private pasteboard back, one line each: its types, the file URL,
        /// the text, and the length of its GIF data.
        fn read_back(name: &str) -> String {
            let script = r#"use framework "AppKit"
on run {theName}
    set board to current application's NSPasteboard's pasteboardWithName:theName
    set theTypes to (board's types()) as list
    set theURL to board's stringForType:"public.file-url"
    set theText to board's stringForType:"public.utf8-plain-text"
    set theData to board's dataForType:"com.compuserve.gif"
    set theLength to 0
    if theData is not missing value then set theLength to theData's |length|()
    board's releaseGlobally()
    set AppleScript's text item delimiters to ","
    return (theTypes as text) & linefeed & (theURL as text) & linefeed & (theText as text) & linefeed & theLength
end run"#;
            let output = Command::new("osascript").arg("-e").arg(script).arg(name).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            String::from_utf8(output.stdout).unwrap()
        }

        fn unique(label: &str) -> String {
            format!("hako-test-{label}-{}", std::process::id())
        }

        #[test]
        fn writes_image_data_and_file_url() {
            let dir = std::env::temp_dir().join(unique("dir"));
            std::fs::create_dir_all(&dir).unwrap();
            let gif = dir.join("cat jam.gif");
            // A 1x1 GIF.
            let bytes: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
            std::fs::write(&gif, bytes).unwrap();

            let name = unique("image");
            MacClipboard::named(&name).write_image(&gif).unwrap();
            let back = read_back(&name);
            std::fs::remove_dir_all(&dir).unwrap();

            let mut lines = back.lines();
            let types = lines.next().unwrap();
            assert!(types.contains("com.compuserve.gif"), "{types}");
            assert!(types.contains("public.file-url"), "{types}");
            assert!(lines.next().unwrap().ends_with("/cat%20jam.gif"));
            assert_eq!(lines.nth(1), Some(bytes.len().to_string().as_str()), "the whole file is on the pasteboard");
        }

        #[test]
        fn writes_text() {
            let name = unique("text");
            MacClipboard::named(&name).write("hello, kura").unwrap();
            assert_eq!(read_back(&name).lines().nth(2), Some("hello, kura"));
        }

        #[test]
        fn rejects_files_that_are_not_images() {
            let file = std::env::temp_dir().join(unique("notes")).with_extension("txt");
            std::fs::write(&file, "not an image").unwrap();
            let result = MacClipboard::named(&unique("bad")).write_image(&file);
            std::fs::remove_file(&file).unwrap();
            assert!(matches!(result, Err(ClipboardError::MimeType(_))));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error(transparent)]
    MimeType(#[from] MimeTypeError),
    #[error("no supported clipboard backend for this platform")]
    UnsupportedPlatform,
    #[error("the clipboard command failed: {0}")]
    Command(String),
    #[error("the file's path isn't valid UTF-8")]
    NonUtf8Path,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
