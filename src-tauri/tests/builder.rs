//! The Docker firmware builder's orchestration, against a fake `docker`
//! executable that records its arguments and simulates builds.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use microbe_lib::firmware::builder::{plan, BuildRequest, Builders, Docker};
use tokio_util::sync::CancellationToken;

const FAKE: &str = r#"#!/bin/bash
LOG="$(dirname "$0")/calls.log"
echo "$*" | head -c 300 >> "$LOG"; echo >> "$LOG"
case "$1" in
  info) exit 0 ;;
  image) [ -f "$(dirname "$0")/image-built" ] && exit 0 || exit 1 ;;
  build) cat > "$(dirname "$0")/dockerfile.txt"; touch "$(dirname "$0")/image-built"; echo "Step 1/2 : FROM ubuntu"; exit 0 ;;
  kill) exit 0 ;;
  run)
    for a in "$@"; do case "$a" in *:/work) WORK="${a%:/work}" ;; esac; done
    SCRIPT="${@: -1}"
    case "$SCRIPT" in *BOARD=FAIL*) echo "error: no such board" >&2; exit 2 ;; esac
    case "$SCRIPT" in *BOARD=SLOW*) sleep 30 ;; esac
    ID=$(echo "$SCRIPT" | sed -n 's|^OUT=/work/out/\(.*\)$|\1|p')
    mkdir -p "$WORK/out/$ID"
    echo "== firmware"
    echo "uf2" > "$WORK/out/$ID/firmware.uf2"
    echo "MICROBE_BUILD_DONE"
    exit 0 ;;
esac
exit 1
"#;

fn fake_docker(dir: &Path) -> PathBuf {
    let exe = dir.join("docker");
    std::fs::write(&exe, FAKE).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

fn request(board: &str) -> BuildRequest {
    BuildRequest { port: "rp2".into(), board: board.into(), variant: None, version: "1.24.0".into(), freeze_project: None, c_modules: None }
}

#[tokio::test]
async fn builds_image_once_then_firmware() {
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let docker = Docker { exe: fake_docker(bin.path()) };
    docker.check().await.unwrap();
    let builders = Builders::builtin().unwrap();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = lines.clone();
    let log = move |s: String| l.lock().unwrap().push(s);

    let p = plan(&builders, &request("RPI_PICO"), work.path(), "b1").unwrap();
    let r = docker.build(&p, &log, &CancellationToken::new()).await.unwrap();
    assert_eq!(r.artifacts, vec![work.path().join("out/b1/firmware.uf2").display().to_string()]);
    let out = lines.lock().unwrap().join("\n");
    assert!(out.contains("building the toolchain image microbe-build-arm:1") && out.contains("Step 1/2") && out.contains("== firmware"), "{out}");
    assert!(!out.contains("MICROBE_BUILD_DONE"), "the completion marker isn't shown");
    assert!(std::fs::read_to_string(bin.path().join("dockerfile.txt")).unwrap().contains("gcc-arm-none-eabi"));

    // The image exists now: no second image build.
    lines.lock().unwrap().clear();
    let p = plan(&builders, &request("RPI_PICO"), work.path(), "b2").unwrap();
    docker.build(&p, &log, &CancellationToken::new()).await.unwrap();
    assert!(!lines.lock().unwrap().join("\n").contains("toolchain image"));
    let calls = std::fs::read_to_string(bin.path().join("calls.log")).unwrap();
    assert_eq!(calls.lines().filter(|l| l.starts_with("build ")).count(), 1);
    assert!(calls.contains("run --rm --name microbe-build-b2"));
}

#[tokio::test]
async fn failed_and_cancelled_builds() {
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    std::fs::write(bin.path().join("image-built"), "").unwrap();
    let docker = Docker { exe: fake_docker(bin.path()) };
    let builders = Builders::builtin().unwrap();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = lines.clone();
    let log = move |s: String| l.lock().unwrap().push(s);

    let p = plan(&builders, &request("FAIL"), work.path(), "f1").unwrap();
    let err = docker.build(&p, &log, &CancellationToken::new()).await.unwrap_err();
    assert!(err.to_string().contains("build failed"), "{err}");
    assert!(lines.lock().unwrap().iter().any(|l| l.contains("no such board")), "stderr is logged");

    let p = plan(&builders, &request("SLOW"), work.path(), "s1").unwrap();
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        c.cancel();
    });
    let started = std::time::Instant::now();
    let err = docker.build(&p, &log, &cancel).await.unwrap_err();
    assert!(err.to_string().contains("cancelled") && started.elapsed().as_secs() < 10, "{err}");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(std::fs::read_to_string(bin.path().join("calls.log")).unwrap().contains("kill microbe-build-s1"));
}
