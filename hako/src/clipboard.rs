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
            let uri = format!("file://{}\n", path.display());

            let mut child = Command::new("wl-copy").arg("--type").arg("text/uri-list").stdin(Stdio::piped()).spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(uri.as_bytes())?;
            }

            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        io::{Read, Write},
        path::Path,
        process::{Command, Output, Stdio},
    };

    use super::{Clipboard, ClipboardError};
    use crate::mime_type::MimeType;

    /// Writes an image as its data and as a file reference, through AppKit's
    /// NSPasteboard (AppleScriptObjC, so no extra crates). Apps that paste
    /// pictures take the data; Messages, Slack and Discord attach the file,
    /// which keeps GIFs animated. Arguments: path, type identifier, pasteboard
    /// name ("" for the general one).
    const WRITE_IMAGE: &str = r#"use framework "AppKit"
use scripting additions
on run argv
    set {thePath, theType, theName} to argv
    set ca to current application
    set theData to ca's NSData's dataWithContentsOfFile:thePath
    if theData is missing value then error "can't read " & thePath
    set theItem to ca's NSPasteboardItem's new()
    theItem's setData:theData forType:theType
    theItem's setString:((ca's NSURL's fileURLWithPath:thePath)'s absoluteString()) forType:"public.file-url"
    if theName is "" then
        set board to ca's NSPasteboard's generalPasteboard()
    else
        set board to ca's NSPasteboard's pasteboardWithName:theName
    end if
    board's clearContents()
    if not (board's writeObjects:{theItem}) then error "the pasteboard refused the image"
end run"#;

    pub(super) struct MacClipboard {
        /// None for the general (system) clipboard; a name for a private one.
        pasteboard: Option<String>,
    }

    impl MacClipboard {
        pub(super) fn general() -> Self {
            Self { pasteboard: None }
        }

        #[cfg(test)]
        pub(super) fn named(name: &str) -> Self {
            Self { pasteboard: Some(name.to_owned()) }
        }
    }

    impl Clipboard for MacClipboard {
        fn write(&self, text: &str) -> Result<(), ClipboardError> {
            if let Some(name) = &self.pasteboard {
                // pbcopy only knows the general pasteboard.
                let script = r#"on run {theText, theName}
    set board to current application's NSPasteboard's pasteboardWithName:theName
    board's clearContents()
    board's setString:theText forType:"public.utf8-plain-text"
end run"#;
                let program = format!("use framework \"AppKit\"\n{script}");
                return check(osascript(&program, &[text, name])?);
            }
            let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
            child.stdin.take().expect("piped").write_all(text.as_bytes())?;
            check(child.wait_with_output()?)
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
            check(osascript(WRITE_IMAGE, &[path, type_id, self.pasteboard.as_deref().unwrap_or("")])?)
        }
    }

    /// Runs an AppleScript program, given on stdin, with `args` as `argv`.
    fn osascript(program: &str, args: &[&str]) -> std::io::Result<Output> {
        let mut child = Command::new("osascript")
            .arg("-")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        child.stdin.take().expect("piped").write_all(program.as_bytes())?;
        child.wait_with_output()
    }

    fn check(output: Output) -> Result<(), ClipboardError> {
        if output.status.success() {
            Ok(())
        } else {
            Err(ClipboardError::Command(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Reads a private pasteboard back: its types, then the file URL.
        fn read_back(name: &str) -> String {
            let script = r#"use framework "AppKit"
on run {theName}
    set board to current application's NSPasteboard's pasteboardWithName:theName
    set theTypes to (board's types()) as list
    set theURL to board's stringForType:"public.file-url"
    set theText to board's stringForType:"public.utf8-plain-text"
    board's releaseGlobally()
    set AppleScript's text item delimiters to ","
    return (theTypes as text) & linefeed & (theURL as text) & linefeed & (theText as text)
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
            std::fs::write(&gif, b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;").unwrap();

            let name = unique("image");
            MacClipboard::named(&name).write_image(&gif).unwrap();
            let back = read_back(&name);
            std::fs::remove_dir_all(&dir).unwrap();

            let mut lines = back.lines();
            let types = lines.next().unwrap();
            assert!(types.contains("com.compuserve.gif"), "{types}");
            assert!(types.contains("public.file-url"), "{types}");
            assert!(lines.next().unwrap().ends_with("/cat%20jam.gif"));
        }

        #[test]
        fn writes_text() {
            let name = unique("text");
            MacClipboard::named(&name).write("hello, kura").unwrap();
            assert_eq!(read_back(&name).lines().nth(2), Some("hello, kura"));
        }

        #[test]
        fn rejects_files_that_are_not_images() {
            let file = std::env::temp_dir().join(unique("notes.txt"));
            std::fs::write(&file, "not an image").unwrap();
            let result = MacClipboard::named(&unique("bad")).write_image(&file);
            std::fs::remove_file(&file).unwrap();
            assert!(matches!(result, Err(ClipboardError::MimeType(_))));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("file has no extension")]
    MissingExtension,
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
