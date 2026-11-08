use std::path::{Path, PathBuf};

use kura_core::{
    Added, Change, ChangeKind, ChangeSeq, Error, ImageFilter, ImageHash, ImageName, Library, LibraryPaths, NewName,
    Source, Tag, ThumbSize, WatchEvent,
};

/// A 4x4 PNG whose pixels depend on `seed`, so different seeds hash differently.
fn png_bytes(seed: u8, alpha: bool) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    if alpha {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([seed, 0, 0, 128]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
    } else {
        image::RgbImage::from_pixel(4, 4, image::Rgb([seed, 0, 0]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
    }
    out.into_inner()
}

fn write_png(dir: &Path, file_name: &str, seed: u8) -> PathBuf {
    let path = dir.join(file_name);
    std::fs::write(&path, png_bytes(seed, false)).unwrap();
    path
}

fn tag(s: &str) -> Tag {
    Tag::parse(s).unwrap()
}

fn name(s: &str) -> ImageName {
    ImageName::parse(s).unwrap()
}

async fn open(root: &Path) -> Library {
    Library::open(LibraryPaths::under(root.join("library"))).await.unwrap()
}

#[tokio::test]
async fn add_tag_rename_delete_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = open(tmp.path()).await;
    let src = write_png(tmp.path(), "Grumpy Cat.png", 1);

    // Add: the name comes from the file, the file is stored under its hash.
    let added = lib.add(Source::path(&src).await.unwrap(), NewName::FromSource, &[tag("cats")], None).await.unwrap();
    let Added::Created(image) = added else { panic!("expected a new image") };
    assert_eq!(image.name().as_str(), "grumpy-cat");
    assert_eq!(image.tags(), &[tag("cats")]);
    let file = lib.path_of(&image);
    assert_eq!(file, tmp.path().join(format!("library/pictures/{}.png", image.hash())));
    assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(&src).unwrap());

    // The same bytes again are not an error: the stored image comes back untouched.
    let again = lib.add(Source::path(&src).await.unwrap(), name("other"), &[], None).await.unwrap();
    assert!(!again.is_created());
    assert_eq!(again.image().name().as_str(), "grumpy-cat");

    // A different image can't take the name, ignoring case...
    let other = write_png(tmp.path(), "grumpy_cat.png", 2);
    let err = lib.add(Source::path(&other).await.unwrap(), name("GRUMPY-CAT"), &[], None).await.unwrap_err();
    assert!(matches!(err, Error::NameTaken(_)));
    // ...but a derived name steps aside instead.
    let derived = lib.add(Source::path(&other).await.unwrap(), NewName::FromSource, &[], None).await.unwrap();
    assert_eq!(derived.image().name().as_str(), "grumpy-cat-2");

    // Tags, idempotently.
    let edited = lib.tag(image.hash(), &[tag("reaction"), tag("cats")]).await.unwrap();
    assert!(edited.changed);
    assert_eq!(edited.image.tags(), &[tag("cats"), tag("reaction")]);
    assert!(!lib.tag(image.hash(), &[tag("cats")]).await.unwrap().changed);
    let tagged = lib.list(&ImageFilter::all().tag(tag("reaction"))).await.unwrap();
    assert_eq!(tagged.len(), 1);
    assert!(lib.list(&ImageFilter::all().tag(tag("dogs"))).await.unwrap().is_empty());

    // Rename only touches the database; names ignore case on lookup.
    let renamed = lib.rename(name("Grumpy-Cat"), name("Not Amused")).await.unwrap();
    assert!(renamed.changed);
    assert!(file.exists());
    assert_eq!(lib.image(name("not amused")).await.unwrap().hash(), image.hash());
    assert!(!lib.rename(image.hash(), name("Not Amused")).await.unwrap().changed);

    // Any copy of the file finds the image.
    let by_content = ImageHash::of_file(&src).await.unwrap();
    assert_eq!(&by_content, image.hash());

    // Untag reports whether anything changed.
    assert!(lib.untag(image.hash(), &tag("cats")).await.unwrap().changed);
    assert!(!lib.untag(image.hash(), &tag("cats")).await.unwrap().changed);

    // Delete removes the file and the tags; deleting again is NotFound.
    let deleted = lib.delete(image.hash()).await.unwrap();
    assert_eq!(deleted.name().as_str(), "Not Amused");
    assert!(!file.exists());
    assert!(matches!(lib.delete(image.hash()).await, Err(Error::NotFound(_))));
    assert_eq!(lib.tags().await.unwrap().len(), 0);
}

#[tokio::test]
async fn rejects_bad_sources_without_leaving_files() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open_with(
        LibraryPaths::under(tmp.path().join("library")),
        kura_core::Limits { max_image_bytes: 1024 },
    )
    .await
    .unwrap();

    let not_an_image = lib.add(Source::bytes("definitely not a png", None), NewName::FromSource, &[], None).await;
    assert!(matches!(not_an_image, Err(Error::UnsupportedMedia)));

    let mut huge = png_bytes(1, false);
    huge.resize(4096, 0);
    let too_large = lib.add(Source::bytes(huge, None), NewName::FromSource, &[], None).await;
    assert!(matches!(too_large, Err(Error::TooLarge { limit: 1024 })));

    let missing = Source::path(tmp.path().join("nope.png")).await;
    assert!(matches!(missing, Err(Error::ReadSource { .. })));

    // Nothing was stored, and staging was cleaned up.
    let pictures = tmp.path().join("library/pictures");
    let leftovers: Vec<_> = std::fs::read_dir(&pictures)
        .unwrap()
        .chain(std::fs::read_dir(pictures.join(".kura-staging")).unwrap())
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != ".kura-staging")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn thumbnails_keep_transparency() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = open(tmp.path()).await;

    let opaque = lib.add(Source::bytes(png_bytes(1, false), None), name("opaque"), &[], None).await.unwrap();
    let clear = lib.add(Source::bytes(png_bytes(2, true), None), name("clear"), &[], None).await.unwrap();

    let opaque_thumb = lib.thumbnail(opaque.image().hash(), ThumbSize::Grid).await.unwrap();
    let clear_thumb = lib.thumbnail(clear.image().hash(), ThumbSize::Grid).await.unwrap();
    assert_eq!(opaque_thumb.media_type().as_str(), "image/jpeg");
    assert_eq!(clear_thumb.media_type().as_str(), "image/png");
    assert!(opaque_thumb.path().exists() && clear_thumb.path().exists());

    // Deleting the image removes its thumbnails.
    lib.delete(clear.image().hash()).await.unwrap();
    assert!(!clear_thumb.path().exists());
}

#[tokio::test]
async fn destroy_refuses_while_open() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = LibraryPaths::under(tmp.path().join("library"));
    let lib = Library::open(paths.clone()).await.unwrap();

    assert!(matches!(paths.clone().destroy(), Err(Error::LibraryInUse)));

    lib.close().await;
    paths.clone().destroy().unwrap();
    assert!(!paths.data.exists() && !paths.pictures.exists());
}

/// A library from before migrations: files named after the image, paths in
/// `images.file_path`, tags without the cascade, and names that collide when
/// case is ignored (the old code only kept *file names* unique).
#[tokio::test]
async fn upgrades_legacy_libraries() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = LibraryPaths::under(tmp.path().join("library"));
    std::fs::create_dir_all(&paths.pictures).unwrap();
    std::fs::create_dir_all(&paths.data).unwrap();

    // An upper-case extension, and a JPEG name on what is really a PNG.
    let first = write_png(&paths.pictures, "cat.PNG", 3);
    let second = write_png(&paths.pictures, "cat-copy.jpeg", 4);
    let first_hash = ImageHash::of_file(&first).await.unwrap();
    let second_hash = ImageHash::of_file(&second).await.unwrap();

    {
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(paths.data.join("kura.db")).create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
        for sql in [
            "CREATE TABLE images (id INTEGER PRIMARY KEY AUTOINCREMENT, hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL, file_path TEXT NOT NULL, created_at TEXT NOT NULL)",
            "CREATE TABLE tags (id INTEGER PRIMARY KEY AUTOINCREMENT,
                image_hash TEXT NOT NULL REFERENCES images(hash), tag TEXT NOT NULL,
                created_at TEXT NOT NULL, UNIQUE(image_hash, tag))",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        for (hash, name, path) in [(&first_hash, "Cat", &first), (&second_hash, "cat", &second)] {
            sqlx::query(
                "INSERT INTO images (hash, name, file_path, created_at) VALUES (?, ?, ?, '2026-01-02 03:04:05')",
            )
            .bind(hash.as_str())
            .bind(name)
            .bind(path.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO tags (image_hash, tag, created_at) VALUES (?, 'old', '2026-01-02 03:04:05')")
            .bind(first_hash.as_str())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    let lib = Library::open(paths.clone()).await.unwrap();

    // The oldest image keeps its name; the clash gets a suffix instead of
    // making the upgrade fail.
    let cat = lib.image(name("Cat")).await.unwrap();
    assert_eq!(cat.hash(), &first_hash);
    assert_eq!(cat.tags(), &[tag("old")]);
    let copy = lib.image(name("cat-2")).await.unwrap();
    assert_eq!(copy.hash(), &second_hash);

    // Files moved to <hash>.<ext>, with the extension taken from the contents.
    assert_eq!(lib.path_of(&cat), paths.pictures.join(format!("{first_hash}.png")));
    assert_eq!(lib.path_of(&copy), paths.pictures.join(format!("{second_hash}.png")));
    assert!(lib.path_of(&cat).exists() && lib.path_of(&copy).exists());
    assert!(!first.exists() && !second.exists());
    assert_eq!(copy.media_type().map(|m| m.as_str()), Some("image/png"));

    // The rebuilt tags table cascades deletes.
    lib.delete(cat.hash()).await.unwrap();
    assert!(lib.tags().await.unwrap().is_empty());

    // Opening again is a no-op.
    lib.close().await;
    let lib = Library::open(paths).await.unwrap();
    assert_eq!(lib.list(&ImageFilter::all()).await.unwrap().len(), 1);
}

/// Legacy rows store absolute paths. A copied or moved library must use its
/// own picture folder and never move files that live somewhere else.
#[tokio::test]
async fn legacy_upgrade_never_touches_files_outside_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    let original_dir = tmp.path().join("original-pictures");
    std::fs::create_dir_all(&original_dir).unwrap();
    let original = write_png(&original_dir, "img-2199.JPG", 5);
    let hash = ImageHash::of_file(&original).await.unwrap();

    // A copy of that library, whose row still points at the original file.
    let paths = LibraryPaths::under(tmp.path().join("copy"));
    std::fs::create_dir_all(&paths.pictures).unwrap();
    std::fs::create_dir_all(&paths.data).unwrap();
    std::fs::copy(&original, paths.pictures.join("img-2199.JPG")).unwrap();
    {
        let options =
            sqlx::sqlite::SqliteConnectOptions::new().filename(paths.data.join("kura.db")).create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE images (id INTEGER PRIMARY KEY AUTOINCREMENT, hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL, file_path TEXT NOT NULL, created_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO images (hash, name, file_path, created_at) VALUES (?, 'img-2199', ?, '2026-05-21 19:42:00')",
        )
        .bind(hash.as_str())
        .bind(original.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    let lib = Library::open(paths.clone()).await.unwrap();
    let image = lib.image(name("img-2199")).await.unwrap();

    assert!(original.exists(), "the original library's file must not move");
    assert_eq!(lib.path_of(&image), paths.pictures.join(format!("{hash}.png")));
    assert!(lib.path_of(&image).exists());
    assert!(!paths.pictures.join("img-2199.JPG").exists());
}

async fn next_change(feed: &mut (impl futures::Stream<Item = kura_core::Result<WatchEvent>> + Unpin)) -> WatchEvent {
    use futures::StreamExt;
    tokio::time::timeout(std::time::Duration::from_secs(5), feed.next())
        .await
        .expect("no change within 5 s")
        .expect("feed ended")
        .unwrap()
}

fn change(event: WatchEvent) -> Change {
    match event {
        WatchEvent::Change(change) => change,
        WatchEvent::Resync => panic!("unexpected resync"),
    }
}

#[tokio::test]
async fn watch_streams_edits_and_resumes() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = open(tmp.path()).await;

    // `None` starts from now: earlier edits aren't replayed.
    let early = lib.add(Source::bytes(png_bytes(1, false), None), name("early"), &[], None).await.unwrap().into_image();
    let mut feed = Box::pin(lib.watch(None).await.unwrap());
    let edits = tokio::spawn({
        let lib = lib.clone();
        let early = early.clone();
        async move {
            let _ = lib.tag(&early, &[tag("x")]).await.unwrap();
            let _ = lib.rename(&early, name("later")).await.unwrap();
            // A no-op edit records nothing.
            let _ = lib.tag(early.hash(), &[tag("x")]).await.unwrap();
            lib.delete(early.hash()).await.unwrap();
        }
    });
    let tagged = change(next_change(&mut feed).await);
    assert_eq!((tagged.kind, tagged.detail.as_deref()), (ChangeKind::Tagged, Some("x")));
    assert_eq!(&tagged.image, early.hash());
    let renamed = change(next_change(&mut feed).await);
    assert_eq!((renamed.kind, renamed.detail.as_deref()), (ChangeKind::Renamed, Some("later")));
    let deleted = change(next_change(&mut feed).await);
    assert_eq!(deleted.kind, ChangeKind::Deleted);
    assert!(tagged.seq < renamed.seq && renamed.seq < deleted.seq);
    edits.await.unwrap();

    // Resuming replays exactly what came after.
    let mut resumed = Box::pin(lib.watch(Some(tagged.seq)).await.unwrap());
    assert_eq!(change(next_change(&mut resumed).await), renamed);
    assert_eq!(change(next_change(&mut resumed).await), deleted);

    // A seq this library never reached (say, from another library) resyncs.
    let mut foreign = Box::pin(lib.watch(Some(ChangeSeq(deleted.seq.0 + 100))).await.unwrap());
    assert_eq!(next_change(&mut foreign).await, WatchEvent::Resync);
}

#[tokio::test]
async fn watch_sees_other_processes() {
    let tmp = tempfile::tempdir().unwrap();
    let server = open(tmp.path()).await;
    // A second handle has its own pool and notifier, like the CLI next to a server.
    let cli = open(tmp.path()).await;

    let mut feed = Box::pin(server.watch(None).await.unwrap());
    let added = cli.add(Source::bytes(png_bytes(1, false), None), name("cat"), &[], None).await.unwrap();
    let event = change(next_change(&mut feed).await);
    assert_eq!((event.kind, &event.image), (ChangeKind::Added, added.image().hash()));
    assert_eq!(event.detail.as_deref(), Some("cat"));
}

#[tokio::test]
async fn sweep_removes_crash_leftovers_and_reports_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = open(tmp.path()).await;
    let root = tmp.path().join("library");

    let kept = lib.add(Source::bytes(png_bytes(1, false), None), name("kept"), &[], None).await.unwrap().into_image();
    let lost = lib.add(Source::bytes(png_bytes(2, false), None), name("lost"), &[], None).await.unwrap().into_image();
    lib.thumbnail(&kept, ThumbSize::Grid).await.unwrap();
    assert!(lib.check().await.unwrap().is_clean());

    // What a crash leaves: an unreferenced image file, an old staging file, a
    // thumbnail of a deleted image. Plus a lost file and one kura didn't write.
    std::fs::remove_file(lib.path_of(&lost)).unwrap();
    let orphan_hash = ImageHash::of_bytes(&png_bytes(3, false));
    let orphan = root.join(format!("pictures/{orphan_hash}.png"));
    std::fs::write(&orphan, png_bytes(3, false)).unwrap();
    let staging = root.join("pictures/.kura-staging/1-2-3.part");
    let file = std::fs::File::create(&staging).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60)).unwrap();
    let fresh_staging = root.join("pictures/.kura-staging/4-5-6.part");
    std::fs::write(&fresh_staging, b"in progress").unwrap();
    let old_thumb = root.join(format!("cache/{orphan_hash}-256.jpg"));
    std::fs::write(&old_thumb, b"x").unwrap();
    let foreign = root.join("pictures/notes.txt");
    std::fs::write(&foreign, b"mine").unwrap();
    std::fs::write(root.join("pictures/.DS_Store"), b"").unwrap();

    let found = lib.check().await.unwrap();
    assert_eq!(found.missing_files, vec![lost.hash().clone()]);
    assert_eq!(found.orphan_files, vec![orphan.clone()]);
    assert_eq!(found.foreign_files, vec![foreign.clone()]);
    assert_eq!(found.stale_staging, vec![staging.clone()]);
    assert_eq!(found.orphan_thumbnails, vec![old_thumb.clone()]);
    assert!(!found.is_clean());

    assert_eq!(lib.sweep().await.unwrap(), found);
    assert!(!orphan.exists() && !staging.exists() && !old_thumb.exists());
    assert!(fresh_staging.exists() && foreign.exists() && lib.path_of(&kept).exists());

    let after = lib.check().await.unwrap();
    assert_eq!(after.missing_files, vec![lost.hash().clone()]);
    assert!(after.orphan_files.is_empty() && after.stale_staging.is_empty() && after.orphan_thumbnails.is_empty());
}

#[tokio::test]
async fn add_keeps_a_given_created_at() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = open(tmp.path()).await;

    let at = kura_core::parse_created_at("2026-05-14 00:29:43").unwrap();
    assert_eq!(at, kura_core::parse_created_at("2026-05-14T00:29:43Z").unwrap());
    let Added::Created(old) =
        lib.add(Source::bytes(png_bytes(1, false), None), name("old"), &[], Some(at)).await.unwrap()
    else {
        panic!("expected a new image");
    };
    assert_eq!(old.created_at(), at);

    // Newest first: the backdated image sorts after one added now.
    let _ = lib.add(Source::bytes(png_bytes(2, false), None), name("new"), &[], None).await.unwrap();
    let names: Vec<_> = lib.list(&ImageFilter::all()).await.unwrap().iter().map(|i| i.name().to_string()).collect();
    assert_eq!(names, ["new", "old"]);

    assert!(kura_core::parse_created_at("yesterday").is_err());
    assert!(kura_core::parse_created_at("2999-01-01T00:00:00Z").is_err());
}
