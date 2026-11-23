use super::*;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::thread;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[test]
fn accepts_http_and_https_course_urls() {
    assert!(validate_url("https://example.com/course.zip").is_ok());
    assert!(validate_url("http://example.com/course.zip").is_ok());
    assert!(matches!(
        validate_url("ftp://example.com/course.zip"),
        Err(CourseImportError::InvalidUrl)
    ));
}

#[test]
fn derives_the_target_from_the_url_file_name() {
    let store = Path::new("store");
    let target = suggested_target(store, "https://example.com/course-1.2.3.zip?token=x")
        .unwrap()
        .expect("a named ZIP");

    assert_eq!(target, store.join("course-1.2.3"));
}

#[test]
fn imports_a_zip_over_http() {
    let zip = fixture_zip("multi-unit-demo");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n",
            zip.len()
        )
        .unwrap();
        stream.write_all(&zip).unwrap();
    });
    let store = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let mut last_progress = None;

    let path = import_course(
        &format!("http://{address}/course.zip"),
        store.path(),
        None,
        &cancel,
        &mut |progress| last_progress = Some(progress),
    )
    .expect("course import should succeed");

    server.join().unwrap();
    assert_eq!(path, store.path().join("course"));
    assert!(path.join("manifest.json").is_file());
    assert_eq!(
        last_progress.expect("progress").stage,
        CourseImportStage::Installing
    );
}

#[test]
fn does_not_overwrite_without_consent() {
    let (address, server) = serve_zips([
        fixture_zip("multi-unit-demo"),
        fixture_zip("multi-unit-demo"),
    ]);
    let store = tempfile::tempdir().unwrap();
    let url = format!("http://{address}/course.zip");
    let cancel = AtomicBool::new(false);
    import_course(&url, store.path(), None, &cancel, &mut |_| {})
        .expect("first import should succeed");

    let error = import_course(&url, store.path(), None, &cancel, &mut |_| {})
        .expect_err("existing directory must not be replaced without consent");
    assert!(matches!(error, CourseImportError::AlreadyExists(_)));
    server.join().unwrap();
}

#[test]
fn overwrites_after_the_new_course_is_staged() {
    let (address, server) = serve_zips([
        fixture_zip("multi-unit-demo"),
        fixture_zip("multi-unit-demo"),
    ]);
    let store = tempfile::tempdir().unwrap();
    let url = format!("http://{address}/course.zip");
    let cancel = AtomicBool::new(false);
    let first = import_course(&url, store.path(), None, &cancel, &mut |_| {})
        .expect("first import should succeed");
    std::fs::write(first.join("old-marker"), "old").unwrap();

    let second = import_course(&url, store.path(), Some(&first), &cancel, &mut |_| {})
        .expect("overwrite should succeed");

    server.join().unwrap();
    assert_eq!(second, first);
    assert!(!second.join("old-marker").exists());
    assert!(second.join("manifest.json").is_file());
}

#[test]
fn changing_download_requires_consent_for_the_new_destination() {
    let (address, server) = serve_zips([
        fixture_zip("multi-unit-demo"),
        fixture_zip("independent-units"),
    ]);
    let url = format!("http://{address}/latest");
    let store = tempfile::tempdir().unwrap();
    let first = store.path().join("multi_unit_demo-1.0.0");
    let second = store.path().join("independent_units-1.0.0");
    for path in [&first, &second] {
        fs::create_dir(path).unwrap();
        fs::write(path.join("notes"), "keep").unwrap();
    }
    let cancel = AtomicBool::new(false);
    let Err(CourseImportError::AlreadyExists(confirmed)) =
        import_course(&url, store.path(), None, &cancel, &mut |_| {})
    else {
        panic!("expected the first course to require confirmation");
    };
    assert_eq!(confirmed, first);
    let result = import_course(&url, store.path(), Some(&confirmed), &cancel, &mut |_| {});
    assert!(matches!(result, Err(CourseImportError::AlreadyExists(path)) if path == second));
    for path in [first, second] {
        assert_eq!(fs::read_to_string(path.join("notes")).unwrap(), "keep");
    }
    server.join().unwrap();
}

fn serve_zips(responses: [Vec<u8>; 2]) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for zip in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                zip.len()
            )
            .unwrap();
            stream.write_all(&zip).unwrap();
        }
    });
    (address, server)
}

fn fixture_zip(name: &str) -> Vec<u8> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test-data")
        .join(name);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_directory(&mut writer, &fixture, "course", options);
    writer.finish().unwrap().into_inner()
}

fn add_directory(
    writer: &mut ZipWriter<Cursor<Vec<u8>>>,
    directory: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            add_directory(writer, &path, &name, options);
        } else {
            writer.start_file(name, options).unwrap();
            writer.write_all(&std::fs::read(path).unwrap()).unwrap();
        }
    }
}
