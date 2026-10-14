//! End-to-end tests against the MicroPython unix port running in a PTY.
//! Skipped when no `micropython` executable is installed.
#![cfg(unix)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{start, wait_for};

use microbe_lib::device_info;
use microbe_lib::fs;
use microbe_lib::platform::PlatformRegistry;
use microbe_lib::progress;
use microbe_lib::repl::session::SessionEvent;

fn nop(_: u64, _: u64) {}

#[tokio::test(flavor = "multi_thread")]
async fn identify_and_files() {
    let Some(h) = start() else { return };
    let p = h.session.profile();
    let mut raw = h.session.raw().await.unwrap();

    let info = device_info::read(&mut raw, &p).await.unwrap();
    assert!(["darwin", "linux"].contains(&info.sys_platform.as_str()), "{info:?}");
    assert!(info.version.starts_with("1."));
    let target = PlatformRegistry::load(None).unwrap().resolve(&info);
    assert_eq!(target.platform, "unix");

    // write / read round trip with binary data and nested dirs
    let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 256) as u8).collect();
    fs::write_in(&mut raw, &p, "./lib/sub/blob.bin", &data, &nop).await.unwrap();
    assert_eq!(std::fs::read(h.dir.path().join("lib/sub/blob.bin")).unwrap(), data);
    assert_eq!(fs::read_in(&mut raw, &p, "./lib/sub/blob.bin", &nop).await.unwrap(), data);

    let list = fs::list_in(&mut raw, &p, "./lib").await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].is_dir && list[0].name == "sub");

    let walk = fs::walk_in(&mut raw, &p, ".", true).await.unwrap();
    let blob = walk.iter().find(|f| f.path == "./lib/sub/blob.bin").unwrap();
    assert_eq!(blob.size, 5000);
    use sha2::Digest;
    assert_eq!(blob.sha256.as_deref(), Some(hex::encode(sha2::Sha256::digest(&data)).as_str()));

    fs::rename_in(&mut raw, &p, "./lib/sub/blob.bin", "./lib/sub/b2.bin").await.unwrap();
    assert!(fs::stat_in(&mut raw, &p, "./lib/sub/blob.bin").await.unwrap().is_none());
    fs::remove_in(&mut raw, &p, "./lib").await.unwrap();
    assert!(!h.dir.path().join("lib").exists());

    let err = fs::read_in(&mut raw, &p, "./missing.py", &nop).await.unwrap_err();
    assert!(err.to_string().contains("ENOENT") || err.to_string().contains("Errno"), "{err}");
    raw.finish().await.unwrap();

    // The terminal saw nothing from the internal ops.
    assert!(!String::from_utf8_lossy(&h.term.lock().unwrap()).contains("raw REPL"));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_with_stdin_and_stop() {
    let Some(h) = start() else { return };
    h.session.run("name = input('name? ')\nprint('hello', name)\n".into(), vec![]).await.unwrap();
    wait_for(&h, || String::from_utf8_lossy(&h.term.lock().unwrap()).contains("name? ")).await;
    h.session.write(b"bob\r").await.unwrap();
    wait_for(&h, || h.events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::RunFinished { .. }))).await;
    let out = String::from_utf8_lossy(&h.term.lock().unwrap()).into_owned();
    assert!(out.contains("hello bob"), "{out:?}");
    assert!(matches!(h.events.lock().unwrap().last(), Some(SessionEvent::RunFinished { error: None })));

    // An infinite loop, interrupted.
    h.events.lock().unwrap().clear();
    h.session.run("import time\nwhile True:\n    time.sleep_ms(10)\n".into(), vec![]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.session.stop(false).await.unwrap();
    wait_for(&h, || h.events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::RunFinished { .. }))).await;
    let ev = h.events.lock().unwrap().last().cloned().unwrap();
    match ev {
        SessionEvent::RunFinished { error: Some(e) } => assert!(e.contains("KeyboardInterrupt"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }

    // Errors are reported.
    h.events.lock().unwrap().clear();
    h.session.run("1/0\n".into(), vec![]).await.unwrap();
    wait_for(&h, || h.events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::RunFinished { .. }))).await;
    assert!(matches!(h.events.lock().unwrap().last(), Some(SessionEvent::RunFinished { error: Some(e) }) if e.contains("ZeroDivisionError")));

    // The friendly REPL still works afterwards.
    h.term.lock().unwrap().clear();
    h.session.write(b"print(6*7)\r").await.unwrap();
    wait_for(&h, || String::from_utf8_lossy(&h.term.lock().unwrap()).contains("42")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn project_sync_only_uploads_changes() {
    let Some(h) = start() else { return };
    let proj = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(proj.path().join("lib")).unwrap();
    std::fs::write(proj.path().join("main.py"), "print('main')\n").unwrap();
    std::fs::write(proj.path().join("lib/util.py"), "X = 1\n").unwrap();
    std::fs::write(proj.path().join("machine.pyi"), "# stub, ignored\n").unwrap();

    let opts = microbe_lib::sync::SyncOptions {
        local_dir: proj.path().display().to_string(),
        remote_dir: "./app".into(),
        ignore: vec![],
        delete_extra: true,
        dry_run: false,
    };
    let r = progress::Reporter::with_sink(Arc::new(|_| {}), "t", "t");
    let first = microbe_lib::sync::sync(&h.session, &opts, &r).await.unwrap();
    assert_eq!(first.uploaded, vec!["./app/lib/util.py", "./app/main.py"]);
    assert_eq!(std::fs::read_to_string(h.dir.path().join("app/main.py")).unwrap(), "print('main')\n");

    let second = microbe_lib::sync::sync(&h.session, &opts, &r).await.unwrap();
    assert!(second.uploaded.is_empty());
    assert_eq!(second.unchanged, 2);

    std::fs::write(proj.path().join("main.py"), "print('changed')\n").unwrap();
    std::fs::remove_file(proj.path().join("lib/util.py")).unwrap();
    let third = microbe_lib::sync::sync(&h.session, &opts, &r).await.unwrap();
    assert_eq!(third.uploaded, vec!["./app/main.py"]);
    assert_eq!(third.deleted, vec!["./app/lib/util.py"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn preludes_and_structured_records() {
    let Some(h) = start() else { return };
    let prelude = "def emit(tag, v):\n    import json\n    print('\\x1e' + tag, json.dumps(v))\n".to_string();
    h.session
        .run("print('before')\nemit('X', {'a': [1, 2]})\nprint('after')\n".into(), vec![prelude])
        .await
        .unwrap();
    wait_for(&h, || h.events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::RunFinished { .. }))).await;
    assert_eq!(*h.records.lock().unwrap(), vec![("X".to_string(), serde_json::json!({"a": [1, 2]}))]);
    let out = String::from_utf8_lossy(&h.term.lock().unwrap()).into_owned();
    assert!(out.contains("before") && out.contains("after") && !out.contains('\x1e'), "{out:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rescue_disables_main_py() {
    let Some(h) = start() else { return };
    std::fs::write(h.dir.path().join("main.py"), "while True:\n    pass\n").unwrap();
    let report = h
        .session
        .rescue(&microbe_lib::repl::session::RescueOptions { reset: false, disable_main: true, window_ms: 2000 })
        .await
        .unwrap();
    assert!(report.reached_repl);
    assert_eq!(report.disabled, vec!["./main.py.disabled"]);
    assert!(!h.dir.path().join("main.py").exists());
    assert!(h.dir.path().join("main.py.disabled").exists());
    // The session is usable afterwards.
    let mut raw = h.session.raw().await.unwrap();
    let out = raw.exec_str("print(1+1)").await;
    assert_eq!(raw.finish_with(out).await.unwrap().trim(), "2");
}
